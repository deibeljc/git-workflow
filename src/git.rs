use anyhow::{anyhow, Context, Result};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

/// Result of a git rebase operation.
pub enum RebaseResult {
    /// Rebase completed successfully.
    Success,
    /// Rebase paused due to a conflict.
    Conflict,
}

/// Wrapper around git CLI operations.
///
/// All git interactions go through this struct. It uses `std::process::Command`
/// with array-form arguments (never shell string concatenation) to prevent
/// command injection.
pub struct Git {
    repo_path: PathBuf,
}

impl Git {
    pub fn new(repo_path: PathBuf) -> Self {
        Self { repo_path }
    }

    /// Find the git repository root from the current directory.
    pub fn discover() -> Result<Self> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .context("failed to execute git")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Not a git repository. Run `git init` first."
            ));
        }

        let path = String::from_utf8(output.stdout)
            .context("git output was not valid UTF-8")?;
        Ok(Self::new(PathBuf::from(path.trim())))
    }

    /// Run a git command and return stdout as a trimmed string.
    /// Uses array-form arguments to prevent command injection.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.repo_path)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .with_context(|| format!("failed to execute git {}", args.first().unwrap_or(&"")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "git {} failed (exit {}): {}",
                args.join(" "),
                output.status.code().unwrap_or(-1),
                stderr.trim()
            ));
        }

        let stdout = String::from_utf8(output.stdout)
            .context("git output was not valid UTF-8")?;
        Ok(stdout.trim().to_string())
    }

    /// Run a git command and return the raw output, regardless of exit code.
    fn run_raw(&self, args: &[&str]) -> Result<std::process::Output> {
        Command::new("git")
            .args(args)
            .current_dir(&self.repo_path)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .with_context(|| format!("failed to execute git {}", args.first().unwrap_or(&"")))
    }

    pub fn repo_path(&self) -> &std::path::Path {
        &self.repo_path
    }

    /// Get the name of the current branch.
    pub fn current_branch(&self) -> Result<String> {
        self.run(&["rev-parse", "--abbrev-ref", "HEAD"])
    }

    /// Resolve a refspec to a full commit SHA.
    pub fn rev_parse(&self, refspec: &str) -> Result<String> {
        self.run(&["rev-parse", refspec])
    }

    /// Resolve a refspec to a short commit SHA.
    pub fn rev_parse_short(&self, refspec: &str) -> Result<String> {
        self.run(&["rev-parse", "--short", refspec])
    }

    /// Check if a branch exists locally.
    pub fn branch_exists(&self, name: &str) -> Result<bool> {
        let output = self.run_raw(&["rev-parse", "--verify", &format!("refs/heads/{name}")])?;
        Ok(output.status.success())
    }

    /// Get all local branch names as a HashSet for efficient lookup.
    pub fn all_local_branches(&self) -> Result<HashSet<String>> {
        let output = self.run(&["branch", "--format=%(refname:short)"])?;
        if output.is_empty() {
            return Ok(HashSet::new());
        }
        Ok(output.lines().map(|s| s.to_string()).collect())
    }

    /// Create a new branch at the specified start point.
    pub fn create_branch(&self, name: &str, start_point: &str) -> Result<()> {
        self.run(&["branch", "--", name, start_point])?;
        Ok(())
    }

    /// Checkout a branch.
    /// Safety: branch names are validated by validate.rs to never start with '-'.
    pub fn checkout(&self, branch: &str) -> Result<()> {
        self.run(&["checkout", branch])?;
        Ok(())
    }

    /// Check if the working tree has uncommitted changes.
    pub fn is_working_tree_clean(&self) -> Result<bool> {
        let status = self.run(&["status", "--porcelain"])?;
        Ok(status.is_empty())
    }

    /// Rebase the current branch onto the specified target.
    /// Returns Success or Conflict (never errors on conflict).
    pub fn rebase(&self, onto: &str) -> Result<RebaseResult> {
        self.rebase_impl(&["rebase", onto])
    }

    /// Rebase the current branch using --onto to only replay commits after `upstream`.
    /// This is needed after squash merges where the old parent's commits are already
    /// in the target via a different (squashed) SHA.
    pub fn rebase_onto(&self, onto: &str, upstream: &str) -> Result<RebaseResult> {
        self.rebase_impl(&["rebase", "--onto", onto, upstream])
    }

    fn rebase_impl(&self, args: &[&str]) -> Result<RebaseResult> {
        let output = self.run_raw(args)?;

        if output.status.success() {
            return Ok(RebaseResult::Success);
        }

        // Check for conflict state by inspecting git's rebase directories
        if self.is_rebase_in_progress() {
            return Ok(RebaseResult::Conflict);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("git rebase failed: {}", stderr.trim()))
    }

    /// Continue an in-progress rebase.
    pub fn rebase_continue(&self) -> Result<RebaseResult> {
        let output = self.run_raw(&["rebase", "--continue"])?;

        if output.status.success() {
            return Ok(RebaseResult::Success);
        }

        if self.is_rebase_in_progress() {
            return Ok(RebaseResult::Conflict);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("git rebase --continue failed: {}", stderr.trim()))
    }

    /// Abort an in-progress rebase.
    pub fn rebase_abort(&self) -> Result<()> {
        if self.is_rebase_in_progress() {
            self.run(&["rebase", "--abort"])?;
        }
        Ok(())
    }

    /// Check if a git rebase is currently in progress.
    pub fn is_rebase_in_progress(&self) -> bool {
        let git_dir = self.repo_path.join(".git");
        git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
    }

    /// Check for unresolved merge conflicts.
    pub fn has_unresolved_conflicts(&self) -> Result<bool> {
        let output = self.run(&["diff", "--name-only", "--diff-filter=U"])?;
        Ok(!output.is_empty())
    }

    /// Compute the merge base of two refs.
    pub fn merge_base(&self, a: &str, b: &str) -> Result<String> {
        self.run(&["merge-base", a, b])
    }

    /// Check if `ancestor` is an ancestor of `descendant`.
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let output =
            self.run_raw(&["merge-base", "--is-ancestor", ancestor, descendant])?;
        Ok(output.status.success())
    }

    /// Check if a local branch has diverged from its remote tracking branch.
    pub fn has_diverged_from_remote(&self, branch: &str) -> Result<bool> {
        let remote_ref = format!("origin/{branch}");
        // Check if remote tracking ref exists
        let remote_exists = self
            .run_raw(&["rev-parse", "--verify", &format!("refs/remotes/{remote_ref}")])?
            .status
            .success();

        if !remote_exists {
            return Ok(false); // No remote tracking, not diverged
        }

        // Check if local is ancestor of remote (not diverged) or vice versa
        let local_sha = self.rev_parse(&format!("refs/heads/{branch}"))?;
        let remote_sha = self.rev_parse(&format!("refs/remotes/{remote_ref}"))?;

        if local_sha == remote_sha {
            return Ok(false); // Same commit
        }

        // If local is ancestor of remote, we're behind (not diverged from push perspective)
        // If remote is ancestor of local, we're ahead (not diverged, can fast-forward push)
        // If neither is ancestor, we're diverged
        let local_is_ancestor = self.is_ancestor(&local_sha, &remote_sha)?;
        let remote_is_ancestor = self.is_ancestor(&remote_sha, &local_sha)?;

        Ok(!local_is_ancestor && !remote_is_ancestor)
    }

    /// Get commits between base and head as (short_sha, subject) pairs.
    pub fn log_oneline(&self, base: &str, head: &str, limit: usize) -> Result<Vec<(String, String)>> {
        let output = self.run(&[
            "log",
            "--reverse",
            "--oneline",
            "--format=%h %s",
            &format!("--max-count={limit}"),
            &format!("{base}..{head}"),
        ])?;
        if output.is_empty() {
            return Ok(vec![]);
        }
        Ok(output
            .lines()
            .map(|line| {
                let (sha, subject) = line.split_once(' ').unwrap_or((line, ""));
                (sha.to_string(), subject.to_string())
            })
            .collect())
    }

    /// Push a branch to origin.
    pub fn push(&self, branch: &str) -> Result<()> {
        self.run(&["push", "origin", branch])?;
        Ok(())
    }

    /// Force push a branch with lease, using an explicit expected SHA for safety.
    pub fn push_force_with_lease(&self, branch: &str, expected_sha: &str) -> Result<()> {
        let lease = format!("--force-with-lease={branch}:{expected_sha}");
        self.run(&["push", "origin", &lease, branch])?;
        Ok(())
    }

    /// Fetch a specific branch from origin.
    pub fn fetch_branch(&self, remote: &str, branch: &str) -> Result<()> {
        self.run(&["fetch", remote, branch])?;
        Ok(())
    }

    /// Update a local ref to match a remote ref without checking it out.
    pub fn update_local_ref(&self, branch: &str, target: &str) -> Result<()> {
        self.run(&["update-ref", &format!("refs/heads/{branch}"), target])?;
        Ok(())
    }

    /// Stash working tree changes with a message tag.
    /// Uses --include-untracked to capture new files.
    pub fn stash_push(&self, message: &str) -> Result<()> {
        self.run(&["stash", "push", "--include-untracked", "-m", message])?;
        Ok(())
    }

    /// List stash entries whose message matches the given string exactly.
    /// Returns a vec of stash indices (e.g. 0, 2, 5) for matching entries,
    /// ordered from most recent to oldest.
    pub fn stash_list_matching(&self, message: &str) -> Result<Vec<usize>> {
        let output = self.run_raw(&["stash", "list", "--format=%gd %gs"])?;
        let stdout = String::from_utf8(output.stdout)
            .context("git stash list output was not valid UTF-8")?;

        if stdout.trim().is_empty() {
            return Ok(vec![]);
        }

        let mut matches = vec![];
        for line in stdout.trim().lines() {
            // Lines look like: "stash@{0} On branch-name: gw:feature-a"
            // The %gd is "stash@{N}" and %gs is the stash subject
            let Some((gd, gs)) = line.split_once(' ') else {
                continue;
            };
            // Check if the stash subject ends with our exact message.
            // git prepends "On <branch>: " to the message, so we match the suffix.
            if gs.ends_with(message) {
                // Parse the index from stash@{N}
                if let Some(idx_str) = gd.strip_prefix("stash@{").and_then(|s| s.strip_suffix('}'))
                {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        matches.push(idx);
                    }
                }
            }
        }
        Ok(matches)
    }

    /// Pop a stash entry by index. Returns whether conflicts occurred.
    /// On clean pop, the stash entry is removed. On conflict, the stash
    /// entry remains and conflict markers are left in the working tree.
    pub fn stash_pop(&self, index: usize) -> Result<bool> {
        let output = self.run_raw(&["stash", "pop", &format!("stash@{{{index}}}")])?;

        if output.status.success() {
            return Ok(false);
        }

        // Check if this was a conflict (stash applied with merge conflicts)
        // vs an actual error (bad stash index, etc.)
        if self.has_unresolved_conflicts()? {
            return Ok(true);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!(
            "git stash pop failed: {}",
            stderr.trim()
        ))
    }

    /// Atomically update multiple refs using git update-ref --stdin.
    /// Each entry is (branch_name, target_sha).
    pub fn update_ref_transaction(&self, updates: &[(String, String)]) -> Result<()> {
        // Validate SHA values to prevent newline injection into the stdin protocol
        for (branch, sha) in updates {
            if !sha.chars().all(|c| c.is_ascii_hexdigit()) || sha.is_empty() {
                return Err(anyhow!("invalid SHA for branch '{branch}': '{sha}'"));
            }
        }

        let mut stdin_content = String::from("start\n");
        for (branch, sha) in updates {
            stdin_content.push_str(&format!("update refs/heads/{branch} {sha}\n"));
        }
        stdin_content.push_str("commit\n");

        let mut child = Command::new("git")
            .args(["update-ref", "--stdin"])
            .current_dir(&self.repo_path)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("failed to spawn git update-ref")?;

        use std::io::Write;
        if let Some(ref mut stdin) = child.stdin {
            stdin
                .write_all(stdin_content.as_bytes())
                .context("failed to write to git update-ref stdin")?;
        }

        let output = child
            .wait_with_output()
            .context("failed to wait for git update-ref")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("git update-ref transaction failed: {}", stderr.trim()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();

        Command::new("git")
            .args(["init"])
            .current_dir(&path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&path)
            .output()
            .unwrap();

        std::fs::write(path.join("README.md"), "# test").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&path)
            .output()
            .unwrap();

        let git = Git::new(path);
        (dir, git)
    }

    #[test]
    fn test_current_branch() {
        let (_dir, git) = setup_test_repo();
        let branch = git.current_branch().unwrap();
        assert!(branch == "main" || branch == "master");
    }

    #[test]
    fn test_create_and_check_branch() {
        let (_dir, git) = setup_test_repo();
        let head = git.rev_parse("HEAD").unwrap();
        git.create_branch("test-branch", &head).unwrap();
        assert!(git.branch_exists("test-branch").unwrap());
        assert!(!git.branch_exists("nonexistent").unwrap());
    }

    #[test]
    fn test_all_local_branches() {
        let (_dir, git) = setup_test_repo();
        let head = git.rev_parse("HEAD").unwrap();
        git.create_branch("feature-a", &head).unwrap();
        git.create_branch("feature-b", &head).unwrap();

        let branches = git.all_local_branches().unwrap();
        assert!(branches.contains("feature-a"));
        assert!(branches.contains("feature-b"));
    }

    #[test]
    fn test_working_tree_clean() {
        let (_dir, git) = setup_test_repo();
        assert!(git.is_working_tree_clean().unwrap());

        std::fs::write(git.repo_path().join("dirty.txt"), "dirty").unwrap();
        assert!(!git.is_working_tree_clean().unwrap());
    }

    #[test]
    fn test_checkout() {
        let (_dir, git) = setup_test_repo();
        let head = git.rev_parse("HEAD").unwrap();
        git.create_branch("test-branch", &head).unwrap();
        git.checkout("test-branch").unwrap();
        assert_eq!(git.current_branch().unwrap(), "test-branch");
    }

    #[test]
    fn test_is_ancestor() {
        let (_dir, git) = setup_test_repo();
        let base = git.rev_parse("HEAD").unwrap();

        git.create_branch("feature", &base).unwrap();
        git.checkout("feature").unwrap();
        std::fs::write(git.repo_path().join("new.txt"), "new").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-m", "add new file"]).unwrap();

        assert!(git.is_ancestor(&base, "feature").unwrap());
        assert!(!git.is_ancestor("feature", &base).unwrap());
    }

    #[test]
    fn test_rebase_success() {
        let (_dir, git) = setup_test_repo();
        let main_branch = git.current_branch().unwrap();
        let base = git.rev_parse("HEAD").unwrap();

        // Create feature branch with a commit
        git.create_branch("feature", &base).unwrap();
        git.checkout("feature").unwrap();
        std::fs::write(git.repo_path().join("feature.txt"), "feature").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-m", "feature commit"]).unwrap();

        // Add a commit to main
        git.checkout(&main_branch).unwrap();
        std::fs::write(git.repo_path().join("main.txt"), "main").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-m", "main commit"]).unwrap();

        // Rebase feature onto main
        git.checkout("feature").unwrap();
        match git.rebase(&main_branch).unwrap() {
            RebaseResult::Success => {} // expected
            RebaseResult::Conflict => panic!("unexpected conflict"),
        }
    }

    #[test]
    fn test_update_ref_transaction() {
        let (_dir, git) = setup_test_repo();
        let head = git.rev_parse("HEAD").unwrap();

        git.create_branch("branch-a", &head).unwrap();
        git.create_branch("branch-b", &head).unwrap();

        // Add a commit to get a different SHA
        git.checkout("branch-a").unwrap();
        std::fs::write(git.repo_path().join("a.txt"), "a").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-m", "commit on a"]).unwrap();
        let new_sha = git.rev_parse("HEAD").unwrap();

        // Reset branch-a back to original
        git.update_ref_transaction(&[("branch-a".to_string(), head.clone())]).unwrap();
        let after = git.rev_parse("refs/heads/branch-a").unwrap();
        assert_eq!(after, head);

        // Verify we can also move it forward
        git.update_ref_transaction(&[("branch-a".to_string(), new_sha.clone())]).unwrap();
        let after = git.rev_parse("refs/heads/branch-a").unwrap();
        assert_eq!(after, new_sha);
    }

    #[test]
    fn test_rebase_conflict_detection() {
        let (_dir, git) = setup_test_repo();
        let main_branch = git.current_branch().unwrap();

        // Create conflicting changes on two branches
        let base = git.rev_parse("HEAD").unwrap();

        // Branch A modifies README
        git.create_branch("branch-a", &base).unwrap();
        git.checkout("branch-a").unwrap();
        std::fs::write(git.repo_path().join("README.md"), "branch-a content").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-m", "branch-a changes README"]).unwrap();

        // Main also modifies README (creates conflict)
        git.checkout(&main_branch).unwrap();
        std::fs::write(git.repo_path().join("README.md"), "main content").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-m", "main changes README"]).unwrap();

        // Rebase branch-a onto main should conflict
        git.checkout("branch-a").unwrap();
        match git.rebase(&main_branch).unwrap() {
            RebaseResult::Conflict => {
                assert!(git.is_rebase_in_progress());
                // Clean up
                git.rebase_abort().unwrap();
                assert!(!git.is_rebase_in_progress());
            }
            RebaseResult::Success => panic!("expected conflict but got success"),
        }
    }

    #[test]
    fn test_has_diverged_no_remote() {
        let (_dir, git) = setup_test_repo();
        let main_branch = git.current_branch().unwrap();
        // No remote set up, so should not be diverged
        assert!(!git.has_diverged_from_remote(&main_branch).unwrap());
    }

    #[test]
    fn test_merge_base() {
        let (_dir, git) = setup_test_repo();
        let base = git.rev_parse("HEAD").unwrap();

        git.create_branch("branch-a", &base).unwrap();
        git.checkout("branch-a").unwrap();
        std::fs::write(git.repo_path().join("a.txt"), "a").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-m", "commit on a"]).unwrap();

        let mb = git.merge_base("branch-a", &base).unwrap();
        assert_eq!(mb, base);
    }

    #[test]
    fn test_multi_ref_transaction_atomicity() {
        let (_dir, git) = setup_test_repo();
        let base = git.rev_parse("HEAD").unwrap();

        git.create_branch("b1", &base).unwrap();
        git.create_branch("b2", &base).unwrap();
        git.create_branch("b3", &base).unwrap();

        // Make a new commit for a different target
        git.checkout("b1").unwrap();
        std::fs::write(git.repo_path().join("new.txt"), "new").unwrap();
        git.run(&["add", "."]).unwrap();
        git.run(&["commit", "-m", "new commit"]).unwrap();
        let new_sha = git.rev_parse("HEAD").unwrap();

        // Move all three branches atomically
        git.update_ref_transaction(&[
            ("b1".to_string(), base.clone()),
            ("b2".to_string(), new_sha.clone()),
            ("b3".to_string(), new_sha.clone()),
        ]).unwrap();

        assert_eq!(git.rev_parse("refs/heads/b1").unwrap(), base);
        assert_eq!(git.rev_parse("refs/heads/b2").unwrap(), new_sha);
        assert_eq!(git.rev_parse("refs/heads/b3").unwrap(), new_sha);
    }

    #[test]
    fn test_is_working_tree_clean_staged() {
        let (_dir, git) = setup_test_repo();
        assert!(git.is_working_tree_clean().unwrap());

        // Staged but not committed
        std::fs::write(git.repo_path().join("staged.txt"), "staged").unwrap();
        git.run(&["add", "staged.txt"]).unwrap();
        assert!(!git.is_working_tree_clean().unwrap());
    }
}

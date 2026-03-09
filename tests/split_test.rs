mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;
use std::fs;

// ============================================================
// Helper: create a plan file
// ============================================================

fn write_plan(_repo: &TestRepo, content: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let plan_path = dir.path().join("split-plan.txt");
    fs::write(&plan_path, content).unwrap();
    // Leak the tempdir so it survives until the test ends
    let path_str = plan_path.to_str().unwrap().to_string();
    std::mem::forget(dir);
    path_str
}

/// Get the full SHA for HEAD or a ref.
fn full_sha(repo: &TestRepo, refspec: &str) -> String {
    repo.git(&["rev-parse", refspec])
}

/// Create a branch with N commits off the current branch, returning full SHAs.
fn create_branch_with_commits(repo: &TestRepo, branch: &str, files: &[(&str, &str, &str)]) -> Vec<String> {
    repo.git(&["checkout", "-b", branch]);
    let mut shas = Vec::new();
    for (name, content, msg) in files {
        repo.commit_file(name, content, msg);
        shas.push(full_sha(repo, "HEAD"));
    }
    shas
}

// ============================================================
// Split untracked branch — basic happy path
// ============================================================

#[test]
fn split_untracked_branch_into_two_buckets() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    let shas = create_branch_with_commits(&repo, "fat-branch", &[
        ("auth.txt", "auth code", "Add auth"),
        ("auth-test.txt", "auth tests", "Add auth tests"),
        ("dash.txt", "dashboard", "Add dashboard"),
        ("dash-test.txt", "dash tests", "Add dashboard tests"),
    ]);

    let plan = format!(
        "pick {} auth\npick {} auth\npick {} dashboard\npick {} dashboard\n",
        shas[0], shas[1], shas[2], shas[3]
    );
    let plan_path = write_plan(&repo, &plan);

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path, "--base", &main_branch])
        .assert()
        .success()
        .stdout(predicate::str::contains("Split complete"))
        .stdout(predicate::str::contains("auth"))
        .stdout(predicate::str::contains("dashboard"));

    // Verify branches were created
    assert!(repo.branch_exists("auth"));
    assert!(repo.branch_exists("dashboard"));

    // Verify original branch still exists (backup)
    assert!(repo.branch_exists("fat-branch"));

    // Verify stack was created
    assert!(repo.stack_toml_exists("fat-branch"));
    let toml = repo.read_stack_toml("fat-branch");
    assert!(toml.contains("name = \"auth\""));
    assert!(toml.contains("name = \"dashboard\""));
    assert!(toml.contains(&format!("base_branch = \"{main_branch}\"")));

    // Verify we're on the root branch
    assert_eq!(repo.current_branch(), "auth");

    // Verify no state file left
    assert!(!repo.state_toml_exists());

    // Verify auth branch has the right files
    repo.git(&["checkout", "auth"]);
    assert!(repo.path.join("auth.txt").exists());
    assert!(repo.path.join("auth-test.txt").exists());
    assert!(!repo.path.join("dash.txt").exists());

    // Verify dashboard branch has all files (stacked on auth)
    repo.git(&["checkout", "dashboard"]);
    assert!(repo.path.join("auth.txt").exists());
    assert!(repo.path.join("dash.txt").exists());
    assert!(repo.path.join("dash-test.txt").exists());
}

// ============================================================
// Split tracked branch in existing stack
// ============================================================

#[test]
fn split_tracked_branch_replaces_in_stack() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a stack with a fat branch
    gw_cmd(&repo.path)
        .args(["stack", "create", "mystack", "--base", &main_branch])
        .assert()
        .success();

    // Add commits to the stack's branch
    repo.commit_file("a.txt", "a", "Add feature A");
    let sha_a = full_sha(&repo, "HEAD");
    repo.commit_file("b.txt", "b", "Add feature B");
    let sha_b = full_sha(&repo, "HEAD");
    repo.commit_file("c.txt", "c", "Add feature C");
    let sha_c = full_sha(&repo, "HEAD");

    let plan = format!(
        "pick {} part-one\npick {} part-two\npick {} part-two\n",
        sha_a, sha_b, sha_c
    );
    let plan_path = write_plan(&repo, &plan);

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("Split complete"));

    // Verify stack was updated — original branch replaced with new ones
    let toml = repo.read_stack_toml("mystack");
    assert!(toml.contains("name = \"part-one\""));
    assert!(toml.contains("name = \"part-two\""));
    // The original "mystack" branch entry should be gone from the branches list
    // (note: the toml file still has name = "mystack" as the stack name)
    let branch_entries: Vec<&str> = toml
        .lines()
        .filter(|l| l.starts_with("name = "))
        .collect();
    // Stack name + part-one + part-two = 3 lines with "name ="
    assert_eq!(branch_entries.len(), 3);
}

// ============================================================
// Precondition: refuses < 2 commits
// ============================================================

#[test]
fn split_refuses_single_commit_branch() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "tiny"]);
    repo.commit_file("only.txt", "only", "Only commit");

    let sha = full_sha(&repo, "HEAD");
    let plan = format!("pick {} a\npick {} b\n", sha, sha);
    let plan_path = write_plan(&repo, &plan);

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path, "--base", &main_branch])
        .assert()
        .failure()
        .stderr(predicate::str::contains("at least 2"));
}

// ============================================================
// Precondition: refuses merge commits
// ============================================================

#[test]
fn split_refuses_branch_with_merge_commits() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create two branches
    repo.git(&["checkout", "-b", "feature-a"]);
    repo.commit_file("a.txt", "a", "commit a");

    repo.git(&["checkout", &main_branch]);
    repo.git(&["checkout", "-b", "feature-merge"]);
    repo.commit_file("b.txt", "b", "commit b");

    // Merge feature-a into feature-merge (creates a merge commit)
    repo.git(&["merge", "feature-a", "--no-edit"]);

    let plan_path = write_plan(&repo, "pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa x\npick bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb y\n");

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path, "--base", &main_branch])
        .assert()
        .failure()
        .stderr(predicate::str::contains("merge commit"));
}

// ============================================================
// Precondition: refuses dirty working tree
// ============================================================

#[test]
fn split_refuses_dirty_tree() {
    let repo = TestRepo::new();

    repo.git(&["checkout", "-b", "dirty-branch"]);
    repo.commit_file("a.txt", "a", "commit a");
    repo.commit_file("b.txt", "b", "commit b");

    // Make tree dirty
    fs::write(repo.path.join("dirty.txt"), "dirty").unwrap();

    let plan_path = write_plan(&repo, "pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa x\npick bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb y\n");

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path])
        .assert()
        .failure()
        .stderr(predicate::str::contains("uncommitted changes"));
}

// ============================================================
// Plan file: rejects plan with mismatched commits
// ============================================================

#[test]
fn split_rejects_plan_with_wrong_commits() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    create_branch_with_commits(&repo, "branch", &[
        ("a.txt", "a", "commit a"),
        ("b.txt", "b", "commit b"),
    ]);

    // Plan with fake SHAs
    let plan = "pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa x\npick bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb y\n";
    let plan_path = write_plan(&repo, plan);

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path, "--base", &main_branch])
        .assert()
        .failure()
        .stderr(predicate::str::contains("don't match"));
}

// ============================================================
// Plan file: rejects plan with single bucket
// ============================================================

#[test]
fn split_rejects_single_bucket_plan() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    let shas = create_branch_with_commits(&repo, "branch", &[
        ("a.txt", "a", "commit a"),
        ("b.txt", "b", "commit b"),
    ]);

    let plan = format!("pick {} only\npick {} only\n", shas[0], shas[1]);
    let plan_path = write_plan(&repo, &plan);

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path, "--base", &main_branch])
        .assert()
        .failure()
        .stderr(predicate::str::contains("at least 2 buckets"));
}

// ============================================================
// Branch name collision
// ============================================================

#[test]
fn split_rejects_existing_branch_name() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a branch named "taken"
    repo.git(&["branch", "taken"]);

    let shas = create_branch_with_commits(&repo, "branch", &[
        ("a.txt", "a", "commit a"),
        ("b.txt", "b", "commit b"),
    ]);

    let plan = format!("pick {} taken\npick {} other\n", shas[0], shas[1]);
    let plan_path = write_plan(&repo, &plan);

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path, "--base", &main_branch])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

// ============================================================
// State guard: blocked during propagation
// ============================================================

#[test]
fn split_blocked_during_propagation() {
    let repo = TestRepo::new();

    repo.write_state_toml(
        r#"
operation = "rebase"
stack = "test"
started_at = "12345"
original_branch = "main"
original_refs = []
completed = []
remaining = []
"#,
    );

    gw_cmd(&repo.path)
        .args(["split", "--plan", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("propagation is in progress"));
}

// ============================================================
// Cherry-pick conflict: abort
// ============================================================

#[test]
fn split_abort_cleans_up() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a branch with 3 commits
    let shas = create_branch_with_commits(&repo, "fat", &[
        ("a.txt", "aaa", "commit a"),
        ("b.txt", "bbb", "commit b"),
        ("c.txt", "ccc", "commit c"),
    ]);

    // Create a conflicting file on main that will conflict with commit a's cherry-pick
    repo.git(&["checkout", &main_branch]);
    repo.commit_file("a.txt", "conflict!", "conflicting a on main");
    repo.git(&["checkout", "fat"]);

    let plan = format!(
        "pick {} first\npick {} second\npick {} second\n",
        shas[0], shas[1], shas[2]
    );
    let plan_path = write_plan(&repo, &plan);

    // This should hit a conflict on first cherry-pick
    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path, "--base", &main_branch])
        .assert()
        .success()
        .stderr(predicate::str::contains("Conflict"));

    // State file should exist
    assert!(repo.state_toml_exists());

    // Abort
    gw_cmd(&repo.path)
        .args(["split", "--abort"])
        .assert()
        .success()
        .stdout(predicate::str::contains("aborted"));

    // Created branches should be cleaned up
    assert!(!repo.branch_exists("first"));
    assert!(!repo.branch_exists("second"));

    // State file should be gone
    assert!(!repo.state_toml_exists());

    // Should be back on original branch
    assert_eq!(repo.current_branch(), "fat");
}

// ============================================================
// Abort idempotency
// ============================================================

#[test]
fn split_abort_when_no_split_in_progress() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["split", "--abort"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No split in progress"));
}

// ============================================================
// Custom stack name with --name
// ============================================================

#[test]
fn split_custom_stack_name() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    let shas = create_branch_with_commits(&repo, "fat", &[
        ("a.txt", "a", "commit a"),
        ("b.txt", "b", "commit b"),
    ]);

    let plan = format!("pick {} part-a\npick {} part-b\n", shas[0], shas[1]);
    let plan_path = write_plan(&repo, &plan);

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path, "--base", &main_branch, "--name", "my-stack"])
        .assert()
        .success()
        .stdout(predicate::str::contains("my-stack"));

    assert!(repo.stack_toml_exists("my-stack"));
}

// ============================================================
// Split with tracked branch that has descendants
// ============================================================

#[test]
fn split_mid_stack_rebases_descendants() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a 3-branch stack: root -> mid -> leaf
    gw_cmd(&repo.path)
        .args(["stack", "create", "mystack", "--base", &main_branch])
        .assert()
        .success();

    // root branch: "mystack"
    repo.commit_file("root.txt", "root", "root commit");

    // mid branch
    gw_cmd(&repo.path)
        .args(["branch", "create", "mid"])
        .assert()
        .success();

    repo.commit_file("mid-a.txt", "a", "mid commit a");
    let sha_a = full_sha(&repo, "HEAD");
    repo.commit_file("mid-b.txt", "b", "mid commit b");
    let sha_b = full_sha(&repo, "HEAD");

    // leaf branch
    gw_cmd(&repo.path)
        .args(["branch", "create", "leaf"])
        .assert()
        .success();

    repo.commit_file("leaf.txt", "leaf", "leaf commit");

    // Go back to mid and split it
    repo.git(&["checkout", "mid"]);

    let plan = format!("pick {} mid-part1\npick {} mid-part2\n", sha_a, sha_b);
    let plan_path = write_plan(&repo, &plan);

    gw_cmd(&repo.path)
        .args(["split", "--plan", &plan_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("Split complete"));

    // Verify stack updated
    let toml = repo.read_stack_toml("mystack");
    assert!(toml.contains("name = \"mid-part1\""));
    assert!(toml.contains("name = \"mid-part2\""));
    assert!(toml.contains("name = \"leaf\""));
    // "mid" should no longer be in the branches list
    // Count how many branch entries there are
    let branch_lines: Vec<&str> = toml.lines().filter(|l| l.trim().starts_with("name = ")).collect();
    // stack name + mystack + mid-part1 + mid-part2 + leaf = 5
    assert_eq!(branch_lines.len(), 5, "Expected 5 name lines, got: {:?}", branch_lines);

    // Verify leaf still has its file (was rebased successfully)
    repo.git(&["checkout", "leaf"]);
    assert!(repo.path.join("leaf.txt").exists());
    assert!(repo.path.join("mid-a.txt").exists());
    assert!(repo.path.join("mid-b.txt").exists());
}

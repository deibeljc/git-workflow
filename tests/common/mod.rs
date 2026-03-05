use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// A test git repository with helper methods for setting up test scenarios.
pub struct TestRepo {
    pub dir: TempDir,
    pub path: PathBuf,
}

impl TestRepo {
    /// Create a fresh git repo with an initial commit.
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();

        git(&path, &["init"]);
        git(&path, &["config", "user.email", "test@test.com"]);
        git(&path, &["config", "user.name", "Test User"]);

        std::fs::write(path.join("README.md"), "# test repo\n").unwrap();
        git(&path, &["add", "."]);
        git(&path, &["commit", "-m", "initial commit"]);

        Self { dir, path }
    }

    /// Run a git command in this repo and return stdout.
    pub fn git(&self, args: &[&str]) -> String {
        git(&self.path, args)
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> String {
        self.git(&["rev-parse", "--abbrev-ref", "HEAD"])
    }

    /// Get the HEAD sha (short).
    pub fn head_short(&self) -> String {
        self.git(&["rev-parse", "--short", "HEAD"])
    }

    /// Create a file and commit it.
    pub fn commit_file(&self, name: &str, content: &str, message: &str) {
        std::fs::write(self.path.join(name), content).unwrap();
        self.git(&["add", name]);
        self.git(&["commit", "-m", message]);
    }

    /// Check if a branch exists.
    pub fn branch_exists(&self, name: &str) -> bool {
        Command::new("git")
            .args(["rev-parse", "--verify", &format!("refs/heads/{name}")])
            .current_dir(&self.path)
            .output()
            .unwrap()
            .status
            .success()
    }

    /// Check if the gw stacks directory exists and has a stack file.
    pub fn stack_toml_exists(&self, name: &str) -> bool {
        self.path
            .join(".git")
            .join("gw")
            .join("stacks")
            .join(format!("{name}.toml"))
            .exists()
    }

    /// Read a stack TOML file's content.
    pub fn read_stack_toml(&self, name: &str) -> String {
        std::fs::read_to_string(
            self.path
                .join(".git")
                .join("gw")
                .join("stacks")
                .join(format!("{name}.toml")),
        )
        .unwrap()
    }

    /// Check if propagation state file exists.
    pub fn state_toml_exists(&self) -> bool {
        self.path
            .join(".git")
            .join("gw")
            .join("state.toml")
            .exists()
    }

    /// Write a propagation state file for testing state guard.
    pub fn write_state_toml(&self, content: &str) {
        let gw_dir = self.path.join(".git").join("gw");
        std::fs::create_dir_all(&gw_dir).unwrap();
        std::fs::write(gw_dir.join("state.toml"), content).unwrap();
    }
}

/// Run gw binary in a given directory and return the Command for assertion.
pub fn gw_cmd(repo_path: &Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("gw").unwrap();
    cmd.current_dir(repo_path);
    cmd
}

/// Run a raw git command and return trimmed stdout.
fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .to_string()
}

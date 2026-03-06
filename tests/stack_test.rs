mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;

// ============================================================
// gw stack create
// ============================================================

#[test]
fn stack_create_basic() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created stack 'auth'"))
        .stdout(predicate::str::contains(&format!("off {main_branch}")));

    // Branch was created and checked out
    assert_eq!(repo.current_branch(), "auth");
    assert!(repo.branch_exists("auth"));

    // TOML was written
    assert!(repo.stack_toml_exists("auth"));
    let toml = repo.read_stack_toml("auth");
    assert!(toml.contains("name = \"auth\""));
    assert!(toml.contains(&format!("base_branch = \"{main_branch}\"")));
}

#[test]
fn stack_create_with_base_flag() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a dev branch
    repo.git(&["branch", "dev"]);

    // Create stack with explicit --base
    gw_cmd(&repo.path)
        .args(["stack", "create", "feature", "--base", "dev"])
        .assert()
        .success()
        .stdout(predicate::str::contains("off dev"));

    let toml = repo.read_stack_toml("feature");
    assert!(toml.contains("base_branch = \"dev\""));

    // Should be on the new branch
    assert_eq!(repo.current_branch(), "feature");
}

#[test]
fn stack_create_duplicate_name_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Try creating the same stack again (from the auth branch)
    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn stack_create_branch_already_exists_fails() {
    let repo = TestRepo::new();

    // Create a branch manually
    repo.git(&["branch", "my-feature"]);

    // Try to create a stack with that name
    gw_cmd(&repo.path)
        .args(["stack", "create", "my-feature"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Branch 'my-feature' already exists"))
        .stderr(predicate::str::contains("gw adopt"));
}

#[test]
fn stack_create_invalid_name_rejected() {
    let repo = TestRepo::new();

    // Path traversal attempt
    gw_cmd(&repo.path)
        .args(["stack", "create", "../../etc"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot contain"));

    // Name starting with dash
    gw_cmd(&repo.path)
        .args(["stack", "create", "--force"])
        .assert()
        .failure();

    // Name with slashes (valid for branch but not stack)
    gw_cmd(&repo.path)
        .args(["stack", "create", "path/to/thing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("path separator"));
}

#[test]
fn stack_create_outside_git_repo_fails() {
    let dir = tempfile::tempdir().unwrap();

    gw_cmd(dir.path())
        .args(["stack", "create", "test"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not a git repository"));
}

// ============================================================
// gw stack delete
// ============================================================

#[test]
fn stack_delete_basic() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    gw_cmd(&repo.path)
        .args(["stack", "delete", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Untracked stack 'auth'"))
        .stdout(predicate::str::contains("auth"));

    // TOML is gone
    assert!(!repo.stack_toml_exists("auth"));

    // But the git branch still exists
    assert!(repo.branch_exists("auth"));
}

#[test]
fn stack_delete_nonexistent_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "delete", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

// ============================================================
// gw stack list
// ============================================================

#[test]
fn stack_list_empty() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No stacks"));
}

#[test]
fn stack_list_shows_stacks() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Go back to main to create another
    repo.git(&["checkout", &main_branch]);

    gw_cmd(&repo.path)
        .args(["stack", "create", "billing"])
        .assert()
        .success();

    repo.git(&["checkout", &main_branch]);

    let output = gw_cmd(&repo.path)
        .args(["stack", "list"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("auth"));
    assert!(stdout.contains("billing"));
    assert!(stdout.contains("1 branch"));
}

// ============================================================
// State guard
// ============================================================

#[test]
fn state_guard_blocks_commands_during_propagation() {
    let repo = TestRepo::new();

    // Write a fake propagation state
    repo.write_state_toml(
        r#"
operation = "rebase"
stack = "auth"
started_at = "2026-03-05T14:30:00Z"
original_branch = "main"
original_refs = []
completed = []
remaining = ["feature-b"]
current = "feature-b"
"#,
    );

    // stack create should be blocked
    gw_cmd(&repo.path)
        .args(["stack", "create", "new-stack"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("propagation is in progress"))
        .stderr(predicate::str::contains("gw rebase --continue"));

    // stack list should be blocked
    gw_cmd(&repo.path)
        .args(["stack", "list"])
        .assert()
        .failure();

    // tree should still work (read-only)
    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success();
}

// ============================================================
// CLI basics
// ============================================================

#[test]
fn help_flag_works() {
    // --help doesn't need a git repo
    let dir = tempfile::tempdir().unwrap();
    assert_cmd::Command::cargo_bin("gw")
        .unwrap()
        .current_dir(dir.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Git stacked branch manager"))
        .stdout(predicate::str::contains("stack"))
        .stdout(predicate::str::contains("branch"))
        .stdout(predicate::str::contains("rebase"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("push"))
        .stdout(predicate::str::contains("log"));
}

#[test]
fn version_flag_works() {
    let dir = tempfile::tempdir().unwrap();
    assert_cmd::Command::cargo_bin("gw")
        .unwrap()
        .current_dir(dir.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("gw"));
}

#[test]
fn no_args_defaults_to_tree() {
    // Inside a git repo, `gw` with no args runs `gw tree`
    let repo = TestRepo::new();
    gw_cmd(&repo.path)
        .assert()
        .success()
        .stdout(predicate::str::contains("No stacks"));
}

#[test]
fn stack_subcommand_no_args_shows_help() {
    let repo = TestRepo::new();
    gw_cmd(&repo.path)
        .args(["stack"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

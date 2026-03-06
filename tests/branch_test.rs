mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;

// ============================================================
// gw branch create
// ============================================================

#[test]
fn branch_create_basic() {
    let repo = TestRepo::new();

    // Create a stack first
    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Now on 'auth' branch, create a child
    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added 'auth-tests' to stack 'auth'"))
        .stdout(predicate::str::contains("Child of 'auth'"));

    // Should be on the new branch
    assert_eq!(repo.current_branch(), "auth-tests");
    assert!(repo.branch_exists("auth-tests"));

    // TOML should have both branches
    let toml = repo.read_stack_toml("auth");
    assert!(toml.contains("name = \"auth\""));
    assert!(toml.contains("name = \"auth-tests\""));
}

#[test]
fn branch_create_chain_of_three() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("auth.txt", "auth work", "auth commit");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    repo.commit_file("tests.txt", "test work", "test commit");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-ui"])
        .assert()
        .success();

    assert_eq!(repo.current_branch(), "auth-ui");

    // TOML should have all three in order
    let toml = repo.read_stack_toml("auth");
    let auth_pos = toml.find("name = \"auth\"").unwrap();
    let tests_pos = toml.find("name = \"auth-tests\"").unwrap();
    let ui_pos = toml.find("name = \"auth-ui\"").unwrap();
    assert!(auth_pos < tests_pos);
    assert!(tests_pos < ui_pos);
}

#[test]
fn branch_create_not_on_tracked_branch_fails() {
    let repo = TestRepo::new();

    // No stack exists, so current branch (main) is not tracked
    gw_cmd(&repo.path)
        .args(["branch", "create", "feature"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not tracked by any gw stack"));
}

#[test]
fn branch_create_not_from_leaf_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    // Go back to auth (not the leaf)
    repo.git(&["checkout", "auth"]);

    gw_cmd(&repo.path)
        .args(["branch", "create", "another"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not the leaf"));
}

#[test]
fn branch_create_duplicate_name_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Create a branch manually with the name we'll try
    repo.git(&["branch", "existing-branch"]);

    gw_cmd(&repo.path)
        .args(["branch", "create", "existing-branch"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn branch_create_invalid_name_rejected() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    gw_cmd(&repo.path)
        .args(["branch", "create", "--force"])
        .assert()
        .failure();
}

// ============================================================
// gw branch remove
// ============================================================

#[test]
fn branch_remove_leaf() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("auth.txt", "auth", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    // Remove the leaf branch
    gw_cmd(&repo.path)
        .args(["branch", "remove", "auth-tests"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 'auth-tests' from stack 'auth'"))
        .stdout(predicate::str::contains("still exists"));

    // Git branch still exists
    assert!(repo.branch_exists("auth-tests"));

    // TOML should only have auth now
    let toml = repo.read_stack_toml("auth");
    assert!(toml.contains("name = \"auth\""));
    assert!(!toml.contains("name = \"auth-tests\""));
}

#[test]
fn branch_remove_root_promotes_next() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("auth.txt", "auth", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    repo.commit_file("tests.txt", "tests", "test work");

    // Go to a branch that won't be affected
    repo.git(&["checkout", "auth-tests"]);

    // Remove the root branch
    gw_cmd(&repo.path)
        .args(["branch", "remove", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("was root"));

    // TOML should only have auth-tests now
    let toml = repo.read_stack_toml("auth");
    assert!(!toml.contains("name = \"auth\"\n\n[[branches"));
    assert!(toml.contains("name = \"auth-tests\""));
}

#[test]
fn branch_remove_middle_reparents() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("a.txt", "a", "commit a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    repo.commit_file("b.txt", "b", "commit b");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-ui"])
        .assert()
        .success();

    repo.commit_file("c.txt", "c", "commit c");

    // Remove middle branch
    gw_cmd(&repo.path)
        .args(["branch", "remove", "auth-tests"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Re-parented 'auth-ui' onto 'auth'"));

    // TOML should have auth and auth-ui
    let toml = repo.read_stack_toml("auth");
    assert!(toml.contains("name = \"auth\""));
    assert!(!toml.contains("name = \"auth-tests\""));
    assert!(toml.contains("name = \"auth-ui\""));
}

#[test]
fn branch_remove_only_branch_suggests_stack_delete() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    gw_cmd(&repo.path)
        .args(["branch", "remove", "auth"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("only branch"))
        .stderr(predicate::str::contains("gw stack delete"));
}

#[test]
fn branch_remove_untracked_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["branch", "remove", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not tracked"));
}

// ============================================================
// Workflow: create stack, add branches, remove, verify state
// ============================================================

#[test]
fn full_branch_lifecycle() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create stack
    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();

    // Add work and child branches
    repo.commit_file("f1.txt", "f1", "feature 1");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-tests"])
        .assert()
        .success();

    repo.commit_file("t1.txt", "t1", "test 1");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-ui"])
        .assert()
        .success();

    repo.commit_file("u1.txt", "u1", "ui 1");

    // Verify stack has 3 branches
    let toml = repo.read_stack_toml("feature");
    assert!(toml.contains("name = \"feature\""));
    assert!(toml.contains("name = \"feature-tests\""));
    assert!(toml.contains("name = \"feature-ui\""));

    // Remove leaf
    gw_cmd(&repo.path)
        .args(["branch", "remove", "feature-ui"])
        .assert()
        .success();

    // Stack should have 2 branches
    let toml = repo.read_stack_toml("feature");
    assert!(toml.contains("name = \"feature\""));
    assert!(toml.contains("name = \"feature-tests\""));
    assert!(!toml.contains("name = \"feature-ui\""));

    // Delete the stack
    gw_cmd(&repo.path)
        .args(["stack", "delete", "feature"])
        .assert()
        .success();

    // All branches still exist
    assert!(repo.branch_exists("feature"));
    assert!(repo.branch_exists("feature-tests"));
    assert!(repo.branch_exists("feature-ui"));

    // No more stacks
    repo.git(&["checkout", &main_branch]);
    gw_cmd(&repo.path)
        .args(["stack", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No stacks"));
}

// ============================================================
// Auto-stash on branch create
// ============================================================

#[test]
fn branch_create_auto_stashes_dirty_work() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    // Create dirty work on auth
    std::fs::write(repo.path.join("dirty.txt"), "wip").unwrap();

    // Branch create should auto-stash the dirty work
    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stashed changes for auth"))
        .stdout(predicate::str::contains("Added 'auth-tests' to stack 'auth'"));

    // Should be on the new branch with a clean working tree
    assert_eq!(repo.current_branch(), "auth-tests");
    assert!(!repo.path.join("dirty.txt").exists());

    // Switch back to auth - should restore the stashed work
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Restored stashed changes for auth"));

    assert!(repo.path.join("dirty.txt").exists());
    assert_eq!(std::fs::read_to_string(repo.path.join("dirty.txt")).unwrap(), "wip");
}

#[test]
fn branch_create_clean_tree_no_stash() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    // Branch create with clean tree - no stash messages
    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stashed").not())
        .stdout(predicate::str::contains("Added 'auth-tests'"));
}

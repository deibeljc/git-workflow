mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;

#[test]
fn tree_no_stacks() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No stacks"));
}

#[test]
fn tree_single_stack_single_branch() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&main_branch))
        .stdout(predicate::str::contains("auth"))
        .stdout(predicate::str::contains("(root)"))
        .stdout(predicate::str::contains("*")); // current branch marker
}

#[test]
fn tree_shows_commit_counts() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    repo.commit_file("a1.txt", "a1", "commit 1");
    repo.commit_file("a2.txt", "a2", "commit 2");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    repo.commit_file("b1.txt", "b1", "test 1");

    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 commits ahead"))
        .stdout(predicate::str::contains("1 commit ahead"));
}

#[test]
fn tree_multiple_stacks() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "a");

    repo.git(&["checkout", &main_branch]);

    gw_cmd(&repo.path)
        .args(["stack", "create", "billing"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "b");

    repo.git(&["checkout", &main_branch]);

    let output = gw_cmd(&repo.path)
        .args(["tree"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("auth"));
    assert!(stdout.contains("billing"));
    // Both stacks should show the base branch
    assert!(stdout.matches(&main_branch).count() >= 2);
}

#[test]
fn tree_highlights_current_branch() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "b");

    // On auth-tests now, should see * marker
    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("*"));
}

#[test]
fn tree_three_branch_stack() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-tests"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "b");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-ui"])
        .assert()
        .success();
    repo.commit_file("c.txt", "c", "c");

    let output = gw_cmd(&repo.path)
        .args(["tree"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();

    // Verify all branches appear
    assert!(stdout.contains("feature"));
    assert!(stdout.contains("feature-tests"));
    assert!(stdout.contains("feature-ui"));
    assert!(stdout.contains("(root)"));
    assert!(stdout.contains(&main_branch));
}

#[test]
fn tree_missing_branch_shows_warning() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "a");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    // Delete the branch outside of gw
    repo.git(&["checkout", "auth"]);
    repo.git(&["branch", "-D", "auth-tests"]);

    let output = gw_cmd(&repo.path)
        .args(["tree"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("[missing]"), "should show missing indicator, got: {stdout}");
}

#[test]
fn tree_works_during_propagation() {
    // Tree is read-only and should work even during active propagation
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Write fake propagation state
    repo.write_state_toml(
        r#"
operation = "rebase"
stack = "auth"
started_at = "12345"
original_branch = "auth"
original_refs = []
completed = []
remaining = []
"#,
    );

    // Tree should still work
    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auth"));
}

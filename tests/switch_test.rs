mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;

#[test]
fn switch_no_stacks() {
    let repo = TestRepo::new();
    // Piped stdin = non-interactive, but no stacks should bail first
    gw_cmd(&repo.path)
        .args(["switch"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No stacks"));
}

#[test]
fn switch_direct_to_tracked_branch() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    // Switch back to auth
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to auth"));

    assert_eq!(repo.current_branch(), "auth");

    // Switch to auth-tests
    gw_cmd(&repo.path)
        .args(["switch", "auth-tests"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to auth-tests"));

    assert_eq!(repo.current_branch(), "auth-tests");
}

#[test]
fn switch_to_current_branch_no_op() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Already on auth
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already on that branch"));
}

#[test]
fn switch_untracked_branch_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    gw_cmd(&repo.path)
        .args(["switch", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not tracked by gw"));
}

#[test]
fn switch_non_interactive_without_arg_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // When run from assert_cmd, stdin is piped (non-interactive)
    gw_cmd(&repo.path)
        .args(["switch"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Interactive mode requires a terminal"));
}

#[test]
fn switch_works_during_propagation() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

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

    // Switch should work even during propagation
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to auth"));
}

#[test]
fn switch_across_stacks() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    repo.git(&["checkout", &main_branch]);

    gw_cmd(&repo.path)
        .args(["stack", "create", "billing"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "billing work");

    // From billing, switch to auth (different stack)
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Switched to auth"));

    assert_eq!(repo.current_branch(), "auth");
}

#[test]
fn switch_to_base_branch() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    // Switch back to base branch (e.g. main)
    gw_cmd(&repo.path)
        .args(["switch", &main_branch])
        .assert()
        .success()
        .stdout(predicate::str::contains(&format!("Switched to {main_branch}")));

    assert_eq!(repo.current_branch(), main_branch);
}

// --- Auto-stash tests ---

#[test]
fn switch_auto_stashes_dirty_work() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "tests work");

    // Create dirty work on auth-tests
    std::fs::write(repo.path.join("dirty.txt"), "wip").unwrap();

    // Switch to auth - should auto-stash
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stashed changes for auth-tests"))
        .stdout(predicate::str::contains("Switched to auth"));

    // dirty.txt should not be on auth
    assert!(!repo.path.join("dirty.txt").exists());

    // Switch back to auth-tests - should auto-unstash
    gw_cmd(&repo.path)
        .args(["switch", "auth-tests"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Restored stashed changes for auth-tests"))
        .stdout(predicate::str::contains("Switched to auth-tests"));

    // dirty.txt should be back
    assert!(repo.path.join("dirty.txt").exists());
    assert_eq!(std::fs::read_to_string(repo.path.join("dirty.txt")).unwrap(), "wip");
}

#[test]
fn switch_clean_branch_no_stash_messages() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    // Switch with clean working tree - no stash messages
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stashed").not())
        .stdout(predicate::str::contains("Restored").not());
}

#[test]
fn switch_same_branch_dirty_no_stash() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    // Create dirty work
    std::fs::write(repo.path.join("dirty.txt"), "wip").unwrap();

    // Switch to same branch - should be no-op, no stash
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already on that branch"))
        .stdout(predicate::str::contains("Stashed").not());

    // dirty.txt should still be there
    assert!(repo.path.join("dirty.txt").exists());
}

#[test]
fn switch_stashes_untracked_files() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    // Create an untracked file (not git-added)
    std::fs::write(repo.path.join("new-file.txt"), "new stuff").unwrap();

    // Switch away - should stash the untracked file
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stashed changes for auth-tests"));

    assert!(!repo.path.join("new-file.txt").exists());

    // Switch back - should restore
    gw_cmd(&repo.path)
        .args(["switch", "auth-tests"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Restored stashed changes for auth-tests"));

    assert!(repo.path.join("new-file.txt").exists());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("new-file.txt")).unwrap(),
        "new stuff"
    );
}

#[test]
fn switch_exact_branch_name_matching() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "features"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-v2"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "v2 work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-v2-hotfix"])
        .assert()
        .success();

    // Create dirty work on feature-v2-hotfix
    std::fs::write(repo.path.join("hotfix.txt"), "fix").unwrap();

    // Switch to feature-v2
    gw_cmd(&repo.path)
        .args(["switch", "feature-v2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stashed changes for feature-v2-hotfix"));

    // Now create dirty work on feature-v2
    std::fs::write(repo.path.join("v2.txt"), "v2 wip").unwrap();

    // Switch to features (root)
    gw_cmd(&repo.path)
        .args(["switch", "features"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stashed changes for feature-v2"));

    // Switch back to feature-v2 - should only restore feature-v2's stash, not feature-v2-hotfix's
    gw_cmd(&repo.path)
        .args(["switch", "feature-v2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Restored stashed changes for feature-v2"));

    assert!(repo.path.join("v2.txt").exists());
    assert!(!repo.path.join("hotfix.txt").exists()); // Should NOT be restored here

    // Switch to feature-v2-hotfix - should restore its own stash
    gw_cmd(&repo.path)
        .args(["switch", "feature-v2-hotfix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Restored stashed changes for feature-v2-hotfix"));

    assert!(repo.path.join("hotfix.txt").exists());
}

#[test]
fn switch_during_propagation_skips_auto_stash() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    // Create dirty work
    std::fs::write(repo.path.join("dirty.txt"), "wip").unwrap();

    // Simulate active propagation
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

    // Switch should work but NOT auto-stash (git checkout will carry untracked files)
    gw_cmd(&repo.path)
        .args(["switch", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Stashed").not())
        .stdout(predicate::str::contains("Switched to auth"));
}
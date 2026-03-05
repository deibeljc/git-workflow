mod common;

use common::{gw_cmd, TestRepo};
use predicates::prelude::*;

/// End-to-end test covering the full workflow:
/// create stack -> add branches -> commit -> rebase propagate ->
/// simulate merge -> sync -> verify state
#[test]
fn full_workflow_create_branch_rebase_sync() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // === Step 1: Create a stack off main ===
    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    assert_eq!(repo.current_branch(), "auth");
    repo.commit_file("auth.txt", "authentication module", "implement auth");

    // === Step 2: Add child branches ===
    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();

    repo.commit_file("auth_test.txt", "auth tests", "add auth tests");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-ui"])
        .assert()
        .success();

    repo.commit_file("auth_ui.txt", "auth UI", "add auth login page");

    // === Step 3: Verify tree shows the stack ===
    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auth"))
        .stdout(predicate::str::contains("auth-tests"))
        .stdout(predicate::str::contains("auth-ui"));

    // === Step 4: Go back to root, add commits, propagate rebase ===
    repo.git(&["checkout", "auth"]);
    repo.commit_file("auth2.txt", "more auth work", "address PR feedback on auth");

    let pre_tests_sha = repo.git(&["rev-parse", "auth-tests"]);
    let pre_ui_sha = repo.git(&["rev-parse", "auth-ui"]);

    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 branches rebased"));

    let post_tests_sha = repo.git(&["rev-parse", "auth-tests"]);
    let post_ui_sha = repo.git(&["rev-parse", "auth-ui"]);

    assert_ne!(pre_tests_sha, post_tests_sha);
    assert_ne!(pre_ui_sha, post_ui_sha);

    // Verify chain integrity after rebase
    assert!(is_ancestor(&repo, "auth", "auth-tests"));
    assert!(is_ancestor(&repo, "auth-tests", "auth-ui"));

    // === Step 5: Simulate auth getting merged into main ===
    repo.git(&["checkout", &main_branch]);
    // Simulate squash merge: apply auth's changes to main
    let merge_base = repo.git(&["merge-base", &main_branch, "auth"]);
    let diff = std::process::Command::new("git")
        .args(["diff", &merge_base, "auth"])
        .current_dir(&repo.path)
        .output()
        .unwrap();
    if !diff.stdout.is_empty() {
        let mut apply = std::process::Command::new("git")
            .args(["apply", "--index"])
            .current_dir(&repo.path)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        apply
            .stdin
            .as_mut()
            .unwrap()
            .write_all(&diff.stdout)
            .unwrap();
        apply.wait().unwrap();
    }
    repo.git(&["commit", "--allow-empty", "-m", "squash merge auth (#1)"]);

    // === Step 6: Sync - should detect merge and promote ===
    repo.git(&["checkout", "auth-tests"]);
    gw_cmd(&repo.path)
        .args(["sync", "--merged", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("'auth' was merged"))
        .stdout(predicate::str::contains("New root: 'auth-tests'"));

    // === Step 7: Verify final state ===
    let toml = repo.read_stack_toml("auth");
    let branch_count = toml.matches("[[branches]]").count();
    assert_eq!(branch_count, 2, "should have 2 branches (auth-tests and auth-ui)");
    assert!(toml.contains("name = \"auth-tests\""));
    assert!(toml.contains("name = \"auth-ui\""));

    // Verify chain integrity after sync
    assert!(is_ancestor(&repo, &main_branch, "auth-tests"));
    assert!(is_ancestor(&repo, "auth-tests", "auth-ui"));

    // Tree should reflect new state
    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auth-tests"))
        .stdout(predicate::str::contains("root"))
        .stdout(predicate::str::contains("auth-ui"));
}

/// Test the adopt -> rebase -> branch remove workflow
#[test]
fn workflow_adopt_then_rebase_then_remove() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create independent branches
    repo.git(&["branch", "feature-a"]);
    repo.git(&["checkout", "feature-a"]);
    repo.commit_file("a.txt", "a", "a work");

    repo.git(&["checkout", &main_branch]);
    repo.git(&["branch", "feature-b"]);
    repo.git(&["checkout", "feature-b"]);
    repo.commit_file("b.txt", "b", "b work");

    repo.git(&["checkout", &main_branch]);
    repo.git(&["branch", "feature-c"]);
    repo.git(&["checkout", "feature-c"]);
    repo.commit_file("c.txt", "c", "c work");

    repo.git(&["checkout", &main_branch]);

    // Adopt into a stack
    gw_cmd(&repo.path)
        .args([
            "adopt",
            "feature-a",
            "feature-b",
            "feature-c",
            "--base",
            &main_branch,
            "--yes",
        ])
        .assert()
        .success();

    // Verify chain
    assert!(is_ancestor(&repo, "feature-a", "feature-b"));
    assert!(is_ancestor(&repo, "feature-b", "feature-c"));

    // Go to root, make a change, propagate
    repo.git(&["checkout", "feature-a"]);
    repo.commit_file("a2.txt", "a2", "more a work");

    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 branches rebased"));

    // Remove middle branch
    gw_cmd(&repo.path)
        .args(["branch", "remove", "feature-b"])
        .assert()
        .success();

    // Verify chain: a -> c (b removed)
    assert!(is_ancestor(&repo, "feature-a", "feature-c"));

    // Stack should have 2 branches
    let toml = repo.read_stack_toml("feature-a");
    assert_eq!(toml.matches("[[branches]]").count(), 2);
    assert!(toml.contains("name = \"feature-a\""));
    assert!(toml.contains("name = \"feature-c\""));
}

/// Test multiple stacks can coexist and operate independently
#[test]
fn multiple_stacks_independent_operations() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create two stacks
    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("auth.txt", "auth", "auth");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();
    repo.commit_file("auth_test.txt", "test", "test");

    repo.git(&["checkout", &main_branch]);

    gw_cmd(&repo.path)
        .args(["stack", "create", "billing"])
        .assert()
        .success();
    repo.commit_file("billing.txt", "billing", "billing");

    // Modify auth stack root
    repo.git(&["checkout", "auth"]);
    repo.commit_file("auth2.txt", "auth2", "more auth");

    let pre_billing_sha = repo.git(&["rev-parse", "billing"]);

    // Rebase auth stack - billing should be unaffected
    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .success();

    let post_billing_sha = repo.git(&["rev-parse", "billing"]);
    assert_eq!(
        pre_billing_sha, post_billing_sha,
        "billing should not be affected by auth rebase"
    );

    // Stack list should show both
    repo.git(&["checkout", &main_branch]);
    let output = gw_cmd(&repo.path)
        .args(["stack", "list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("auth"));
    assert!(stdout.contains("billing"));
}

/// Test error recovery: dirty tree prevents destructive operations
#[test]
fn error_recovery_dirty_tree() {
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

    // Dirty the tree
    repo.git(&["checkout", "auth"]);
    std::fs::write(repo.path.join("dirty.txt"), "dirty").unwrap();

    // All destructive operations should fail with clear message
    gw_cmd(&repo.path)
        .args(["rebase"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("uncommitted changes"));

    gw_cmd(&repo.path)
        .args(["sync"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("uncommitted changes"));

    // But read-only operations should work fine
    gw_cmd(&repo.path)
        .args(["tree"])
        .assert()
        .success();

    gw_cmd(&repo.path)
        .args(["stack", "list"])
        .assert()
        .success();
}

fn is_ancestor(repo: &TestRepo, ancestor: &str, descendant: &str) -> bool {
    std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(&repo.path)
        .output()
        .unwrap()
        .status
        .success()
}

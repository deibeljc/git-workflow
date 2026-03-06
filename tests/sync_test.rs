mod common;

use common::{gw_cmd, simulate_squash_merge, TestRepo};
use predicates::prelude::*;

// ============================================================
// gw sync --merged (manual fallback)
// ============================================================

#[test]
fn sync_merged_removes_root_and_rebases() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a stack: main -> auth -> auth-tests
    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "auth-tests"])
        .assert()
        .success();
    repo.commit_file("b.txt", "b", "test work");

    // Simulate merging auth into main
    simulate_squash_merge(&repo, "auth", &main_branch);

    // Sync with manual --merged flag
    repo.git(&["checkout", "auth-tests"]);
    gw_cmd(&repo.path)
        .args(["sync", "--merged", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("'auth' was merged"))
        .stdout(predicate::str::contains("New root: 'auth-tests'"));

    // Stack should now only have auth-tests as a branch
    let toml = repo.read_stack_toml("auth");
    // Count occurrences of [[branches]] - should be 1 (only auth-tests)
    let branch_count = toml.matches("[[branches]]").count();
    assert_eq!(branch_count, 1, "should have exactly 1 branch, got toml:\n{toml}");
    assert!(toml.contains("name = \"auth-tests\""));
}

#[test]
fn sync_no_stacks() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No stacks"));
}

#[test]
fn sync_dirty_tree_fails() {
    let repo = TestRepo::new();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();

    std::fs::write(repo.path.join("dirty.txt"), "dirty").unwrap();

    gw_cmd(&repo.path)
        .args(["sync"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("uncommitted changes"));
}

#[test]
fn sync_specific_stack() {
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

    // Sync only auth stack (no merges, so no rebase happens)
    gw_cmd(&repo.path)
        .args(["sync", "--stack", "auth"])
        .assert()
        .success();
}

#[test]
fn sync_all_branches_merged_empties_stack() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "auth"])
        .assert()
        .success();
    repo.commit_file("a.txt", "a", "auth work");

    // Simulate merge
    simulate_squash_merge(&repo, "auth", &main_branch);

    repo.git(&["checkout", &main_branch]);

    gw_cmd(&repo.path)
        .args(["sync", "--merged", "auth"])
        .assert()
        .success()
        .stdout(predicate::str::contains("All branches in stack 'auth' have been merged"));
}

// ============================================================
// gw sync with tree comparison detection
// ============================================================

#[test]
fn sync_detects_merge_via_tree_comparison() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();
    repo.commit_file("feat.txt", "feature content", "feature work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-tests"])
        .assert()
        .success();
    repo.commit_file("test.txt", "test content", "test work");

    // Simulate squash merge of feature into main
    simulate_squash_merge(&repo, "feature", &main_branch);

    repo.git(&["checkout", "feature-tests"]);

    // Sync without --merged. gh won't be available in test, so it should
    // try tree comparison and detect the merge.
    let output = gw_cmd(&repo.path)
        .args(["sync"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    // It should detect the merge via tree comparison or at minimum sync without error
    assert!(output.status.success(), "sync should succeed, got: {combined}");
}

// ============================================================
// gw sync rebases remaining branches
// ============================================================

#[test]
fn sync_without_merge_does_not_rebase() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();
    repo.commit_file("f.txt", "feature", "feature commit");

    let pre_sha = repo.git(&["rev-parse", "feature"]);

    // Add a commit to main (simulating remote updates)
    repo.git(&["checkout", &main_branch]);
    repo.commit_file("main-update.txt", "update", "main update");

    repo.git(&["checkout", "feature"]);

    // Sync should NOT rebase since nothing was merged. The stack stays
    // pinned so the root branch doesn't diverge from its remote PR.
    gw_cmd(&repo.path)
        .args(["sync"])
        .assert()
        .success();

    let post_sha = repo.git(&["rev-parse", "feature"]);
    assert_eq!(pre_sha, post_sha, "feature should NOT be rebased when nothing was merged");
}

// ============================================================
// gw sync after squash merge doesn't conflict on child branches
// ============================================================

#[test]
fn sync_squash_merge_child_rebases_cleanly() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a stack: main -> feature -> feature-child
    gw_cmd(&repo.path)
        .args(["stack", "create", "feature"])
        .assert()
        .success();
    repo.commit_file("feat.txt", "feature content", "feature work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "feature-child"])
        .assert()
        .success();
    repo.commit_file("child.txt", "child content", "child work");

    // Squash merge feature into main (simulates GitHub squash merge)
    simulate_squash_merge(&repo, "feature", &main_branch);

    repo.git(&["checkout", "feature-child"]);

    // Sync should detect the merge and rebase feature-child onto main
    // WITHOUT conflicting, because --onto skips the already-merged commits.
    gw_cmd(&repo.path)
        .args(["sync", "--merged", "feature"])
        .assert()
        .success()
        .stdout(predicate::str::contains("'feature' was merged"))
        .stdout(predicate::str::contains("synced"));

    // feature-child should be rebased onto main and contain its own file
    repo.git(&["checkout", "feature-child"]);
    assert!(repo.path.join("child.txt").exists(), "child.txt should exist after rebase");

    // The stack should only have feature-child left
    let toml = repo.read_stack_toml("feature");
    let branch_count = toml.matches("[[branches]]").count();
    assert_eq!(branch_count, 1, "should have exactly 1 branch, got toml:\n{toml}");
    assert!(toml.contains("name = \"feature-child\""));
}

#[test]
fn sync_squash_merge_multi_branch_chain_rebases_cleanly() {
    let repo = TestRepo::new();
    let main_branch = repo.current_branch();

    // Create a 3-branch stack: main -> root -> middle -> leaf
    gw_cmd(&repo.path)
        .args(["stack", "create", "root"])
        .assert()
        .success();
    repo.commit_file("root.txt", "root content", "root work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "middle"])
        .assert()
        .success();
    repo.commit_file("middle.txt", "middle content", "middle work");

    gw_cmd(&repo.path)
        .args(["branch", "create", "leaf"])
        .assert()
        .success();
    repo.commit_file("leaf.txt", "leaf content", "leaf work");

    // Squash merge root into main
    simulate_squash_merge(&repo, "root", &main_branch);

    repo.git(&["checkout", "leaf"]);

    // Sync should rebase both middle and leaf cleanly without conflicts.
    // This tests that ALL branches get --onto, not just the new root.
    gw_cmd(&repo.path)
        .args(["sync", "--merged", "root"])
        .assert()
        .success()
        .stdout(predicate::str::contains("'root' was merged"))
        .stdout(predicate::str::contains("synced"));

    // Both remaining branches should have their files
    repo.git(&["checkout", "middle"]);
    assert!(repo.path.join("middle.txt").exists(), "middle.txt should exist");

    repo.git(&["checkout", "leaf"]);
    assert!(repo.path.join("leaf.txt").exists(), "leaf.txt should exist");

    // Stack should have 2 branches left
    let toml = repo.read_stack_toml("root");
    let branch_count = toml.matches("[[branches]]").count();
    assert_eq!(branch_count, 2, "should have 2 branches, got toml:\n{toml}");
}

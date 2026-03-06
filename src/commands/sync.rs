use anyhow::Result;

use crate::cli::SyncArgs;
use crate::context::Ctx;
use crate::gh;
use crate::propagation::{self, PropagationResult};
use crate::state::Operation;
use crate::ui;

pub fn run(args: SyncArgs, ctx: &Ctx) -> Result<()> {
    ctx.require_clean_tree()?;

    let original_branch = ctx.git.current_branch()?;

    // Remember which stack we're in before sync modifies anything
    let original_stack_name = ctx
        .find_stack_for_branch(&original_branch)?
        .map(|s| s.name);

    let stacks = if let Some(ref name) = args.stack {
        vec![ctx.load_stack(name)?]
    } else {
        ctx.load_all_stacks()?
    };

    if stacks.is_empty() {
        ui::info("No stacks to sync.");
        return Ok(());
    }

    // Batch PR status once for all merge detection
    let pr_map = gh::batch_pr_status();

    let mut synced_bases = std::collections::HashSet::new();

    for mut stack in stacks {
        let base = &stack.base_branch;

        // Fetch and update base branch (once per unique base)
        if synced_bases.insert(base.clone()) {
            ui::info(&format!("Fetching {base}..."));
            match ctx.git.fetch_branch("origin", base) {
                Ok(_) => {
                    if let Err(e) = ctx.git.update_local_ref(base, &format!("origin/{base}")) {
                        ui::warn(&format!("Could not update local ref for {base}: {e}"));
                    }
                }
                Err(_) => {
                    ui::warn(&format!(
                        "Could not fetch origin/{base}. Continuing with local state."
                    ));
                }
            }
        }

        if stack.branches.is_empty() {
            continue;
        }

        // Check for merged root branches, tracking the last one removed
        // so we can use it as the upstream for --onto rebases.
        let mut merged_any = false;
        let mut last_merged_branch: Option<String> = None;
        loop {
            let root = match stack.branches.first() {
                Some(b) => b.name.clone(),
                None => break,
            };

            let is_merged = if let Some(ref merged_branch) = args.merged {
                *merged_branch == root
            } else {
                detect_merged(ctx, &root, &stack.base_branch, &pr_map)?
            };

            if is_merged {
                ui::info(&format!(
                    "Detected: '{root}' was merged into {}",
                    stack.base_branch
                ));
                last_merged_branch = Some(root.clone());
                stack.branches.remove(0);
                merged_any = true;

                if stack.branches.is_empty() {
                    ui::info(&format!(
                        "All branches in stack '{}' have been merged!",
                        stack.name
                    ));
                    ctx.save_stack(&stack)?;
                    break;
                }

                let new_root = &stack.branches[0].name;
                ui::info(&format!("New root: '{new_root}'"));

                if args.merged.is_some() {
                    break;
                }
            } else {
                break;
            }
        }

        if merged_any {
            ctx.save_stack(&stack)?;
        }

        // Only rebase if a branch was merged and removed. If nothing was merged,
        // the stack stays pinned to its current base commit so the root branch
        // doesn't diverge from its remote (which would force-push an open PR).
        if merged_any && !stack.branches.is_empty() {
            let branches: Vec<String> =
                stack.branches.iter().map(|b| b.name.clone()).collect();

            let mut targets = Vec::new();
            for branch_name in &branches {
                let parent = stack
                    .parent_of(branch_name)
                    .expect("branch should have a parent in its stack");
                targets.push(parent);
            }

            // After a squash merge, ALL branches need --onto with their
            // current (pre-rebase) parent as the upstream. This ensures each
            // branch only replays its own unique commits:
            //   git rebase --onto <new_parent> <old_parent> <branch>
            //
            // Without this, after the first branch gets rebased to new SHAs,
            // subsequent branches can't find the fork point and replay
            // already-merged commits, causing conflicts.
            let upstream_overrides: Vec<Option<String>> = branches
                .iter()
                .map(|branch_name| {
                    // Resolve the current parent to a SHA before any rebasing
                    let parent = stack.parent_of(branch_name).unwrap();
                    ctx.git.rev_parse(&parent).ok()
                })
                .collect();

            ui::info(&format!(
                "Rebasing {} branch{} onto {}...",
                branches.len(),
                if branches.len() == 1 { "" } else { "es" },
                stack.base_branch
            ));

            match propagation::start_with_upstreams(
                ctx,
                Operation::Sync,
                &stack.name,
                &branches,
                &targets,
                &upstream_overrides,
            )? {
                PropagationResult::Success { rebased_count } => {
                    ui::success(&format!(
                        "Stack '{}' synced. {rebased_count} branch{} rebased.",
                        stack.name,
                        if rebased_count == 1 { "" } else { "es" }
                    ));
                }
                PropagationResult::Conflict { branch } => {
                    ui::warn(&format!(
                        "Conflict while syncing stack '{}' at branch '{branch}'.",
                        stack.name
                    ));
                    ui::info("Resolve conflicts and run `gw rebase --continue`.");
                    return Ok(());
                }
            }
        }
    }

    // Smart checkout: if we were on a branch that got merged, switch to
    // the next branch in its stack. If the whole stack was merged, go to base.
    let still_tracked = ctx.find_stack_for_branch(&original_branch)?.is_some();
    if still_tracked {
        // Our branch is still in a stack, go back to it
        let _ = ctx.git.checkout(&original_branch);
    } else if let Some(ref stack_name) = original_stack_name {
        // Our branch was merged. Check OUR stack specifically for remaining branches.
        let our_stack = ctx.load_stack(stack_name).ok();
        let next_branch = our_stack
            .as_ref()
            .and_then(|s| s.branches.first())
            .map(|b| b.name.clone());

        match next_branch {
            Some(branch) => {
                let _ = ctx.git.checkout(&branch);
                ui::info(&format!("Switched to '{branch}' (next in stack)"));
            }
            None => {
                let base = ctx.default_base_branch().unwrap_or_else(|_| "main".to_string());
                let _ = ctx.git.checkout(&base);
                ui::info(&format!("Switched to '{base}' (all branches merged)"));
            }
        }
    } else {
        // Wasn't in any stack, just go to base
        let base = ctx.default_base_branch().unwrap_or_else(|_| "main".to_string());
        let _ = ctx.git.checkout(&base);
    }

    Ok(())
}

/// Detect if a branch has been merged into the base branch.
/// Uses batched gh PR data first, falls back to tree comparison.
fn detect_merged(
    ctx: &Ctx,
    branch: &str,
    base: &str,
    pr_map: &std::collections::HashMap<String, gh::PrInfo>,
) -> Result<bool> {
    // Check batched gh data first (no extra subprocess)
    if gh::is_branch_merged(pr_map, branch) {
        return Ok(true);
    }

    // Try tree comparison (commit-tree + cherry)
    if let Ok(result) = detect_merged_via_tree(ctx, branch, base) {
        return Ok(result);
    }

    Ok(false)
}

/// Detect merge via tree comparison.
fn detect_merged_via_tree(ctx: &Ctx, branch: &str, base: &str) -> Result<bool> {
    let merge_base = ctx.git.merge_base(branch, base)?;

    let tree = ctx.git.run(&["rev-parse", &format!("{branch}^{{tree}}")])?;
    let synthetic = ctx.git.run(&[
        "commit-tree",
        &tree,
        "-p",
        &merge_base,
        "-m",
        "synthetic squash for merge detection",
    ])?;

    let cherry_output = ctx.git.run(&["cherry", base, &synthetic])?;

    Ok(cherry_output.lines().any(|line| line.starts_with('-')))
}

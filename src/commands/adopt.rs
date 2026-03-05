use anyhow::{bail, Result};

use crate::cli::AdoptArgs;
use crate::context::Ctx;
use crate::state::{BranchEntry, StackConfig};
use crate::ui;
use crate::validate;

pub fn run(args: AdoptArgs, ctx: &Ctx) -> Result<()> {
    // Validate all branch names
    for name in &args.branches {
        validate::validate_branch_name(name)?;
    }

    if args.branches.len() < 1 {
        bail!("At least one branch is required.");
    }

    // Verify all branches exist
    let existing = ctx.git.all_local_branches()?;
    for name in &args.branches {
        if !existing.contains(name) {
            bail!("Branch '{name}' does not exist.");
        }
    }

    // Verify none are already tracked
    let stacks = ctx.load_all_stacks()?;
    for name in &args.branches {
        for stack in &stacks {
            if stack.branch_index(name).is_some() {
                bail!(
                    "Branch '{name}' is already tracked by stack '{}'.",
                    stack.name
                );
            }
        }
    }

    // Determine base branch
    let base_branch = match &args.base {
        Some(b) => {
            if !existing.contains(b) {
                bail!("Base branch '{b}' does not exist.");
            }
            b.clone()
        }
        None => infer_base_branch(ctx, &args.branches[0])?,
    };

    // Determine stack name
    let stack_name = match &args.name {
        Some(n) => {
            validate::validate_stack_name(n)?;
            n.clone()
        }
        None => {
            // Use the first branch name, but sanitized for stack name
            // (replace slashes with hyphens since stack names can't have slashes)
            args.branches[0].replace('/', "-")
        }
    };

    if ctx.stack_exists(&stack_name) {
        bail!("Stack '{stack_name}' already exists.");
    }

    // Check if branches are already chained in the correct order
    let needs_rebase = !branches_are_chained(ctx, &base_branch, &args.branches)?;

    if needs_rebase {
        if !ctx.git.is_working_tree_clean()? {
            bail!("You have uncommitted changes. Commit or stash before running this command.");
        }

        if !args.yes {
            if !ui::confirm(
                "This will rebase branches to form a chain. Continue?",
                false,
            ) {
                ui::info("Aborted.");
                return Ok(());
            }
        }

        let original_branch = ctx.git.current_branch()?;

        // Rebase each branch onto the previous one
        // First branch rebases onto base, second onto first, etc.
        let mut onto = base_branch.clone();
        for name in &args.branches {
            ctx.git.checkout(name)?;
            match ctx.git.rebase(&onto)? {
                crate::git::RebaseResult::Success => {}
                crate::git::RebaseResult::Conflict => {
                    ctx.git.rebase_abort()?;
                    ctx.git.checkout(&original_branch)?;
                    bail!(
                        "Rebase of '{name}' onto '{onto}' would cause conflicts.\n\
                         Resolve conflicts manually and try again."
                    );
                }
            }
            onto = name.clone();
        }

        ctx.git.checkout(&original_branch)?;
        ui::info("Rebased branches into a chain.");
    }

    // Create the stack
    let config = StackConfig {
        name: stack_name.clone(),
        base_branch,
        branches: args
            .branches
            .iter()
            .map(|name| BranchEntry {
                name: name.clone(),
            })
            .collect(),
    };

    ctx.save_stack(&config)?;

    // Print result
    ui::success(&format!("Created stack '{stack_name}'"));
    let chain: Vec<&str> = args.branches.iter().map(|s| s.as_str()).collect();
    ui::info(&format!(
        "{} (root) -> {}",
        chain[0],
        chain[1..].join(" -> ")
    ));

    Ok(())
}

/// Infer the base branch. Checks config first, then computes merge-base against well-known names.
fn infer_base_branch(ctx: &Ctx, first_branch: &str) -> Result<String> {
    // If a default base is configured, try that first
    let config = ctx.load_config()?;
    if let Some(ref configured_base) = config.default_base {
        let existing = ctx.git.all_local_branches()?;
        if existing.contains(configured_base) {
            return Ok(configured_base.clone());
        }
    }

    let candidates = ["dev", "develop", "main", "master"];
    let existing = ctx.git.all_local_branches()?;

    let mut best: Option<(String, String)> = None; // (branch_name, merge_base_sha)

    for candidate in &candidates {
        if !existing.contains(*candidate) {
            continue;
        }

        if let Ok(mb) = ctx.git.merge_base(first_branch, candidate) {
            match &best {
                None => best = Some((candidate.to_string(), mb)),
                Some((_, best_mb)) => {
                    // Pick the candidate whose merge-base is closest to first_branch
                    // (i.e., the one that is an ancestor of the other merge-base)
                    if ctx.git.is_ancestor(best_mb, &mb).unwrap_or(false) {
                        best = Some((candidate.to_string(), mb));
                    }
                }
            }
        }
    }

    match best {
        Some((name, _)) => Ok(name),
        None => bail!(
            "Could not infer base branch. Use --base to specify it explicitly.\n\
             Looked for: {}",
            candidates.join(", ")
        ),
    }
}

/// Check if branches are already chained in the given order.
/// A chain means: base is ancestor of branches[0], branches[0] is ancestor of branches[1], etc.
fn branches_are_chained(ctx: &Ctx, base: &str, branches: &[String]) -> Result<bool> {
    let mut prev = base.to_string();
    for branch in branches {
        if !ctx.git.is_ancestor(&prev, branch)? {
            return Ok(false);
        }
        prev = branch.clone();
    }
    Ok(true)
}

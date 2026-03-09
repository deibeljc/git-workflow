use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::cli::SplitArgs;
use crate::context::Ctx;
use crate::git::CherryPickResult;
use crate::propagation::{self, PropagationResult};
use crate::state::{
    ActiveState, BranchEntry, Operation, SplitBucket, SplitState, StackConfig,
};
use crate::ui;
use crate::validate;

/// A bucket of commits destined for a single branch.
#[derive(Debug)]
pub struct Bucket {
    pub branch_name: String,
    pub commits: Vec<String>, // full SHAs, in application order
}

/// The interface between input (TUI or --plan file) and the execution engine.
#[derive(Debug)]
pub struct SplitPlan {
    pub buckets: Vec<Bucket>,
}

impl SplitPlan {
    /// Validate a split plan for correctness.
    pub fn validate(&self) -> Result<()> {
        if self.buckets.len() < 2 {
            bail!(
                "Split requires at least 2 buckets (branches), got {}",
                self.buckets.len()
            );
        }

        // Check for empty buckets
        for bucket in &self.buckets {
            if bucket.commits.is_empty() {
                bail!("Bucket '{}' has no commits assigned", bucket.branch_name);
            }
        }

        // Check for duplicate branch names
        let mut seen_names = HashSet::new();
        for bucket in &self.buckets {
            if !seen_names.insert(&bucket.branch_name) {
                bail!("Duplicate bucket name '{}'", bucket.branch_name);
            }
        }

        // Check for duplicate commits across buckets
        let mut seen_shas = HashSet::new();
        for bucket in &self.buckets {
            for sha in &bucket.commits {
                if !seen_shas.insert(sha.as_str()) {
                    bail!("Commit {sha} appears in multiple buckets");
                }
            }
        }

        // Validate branch names
        for bucket in &self.buckets {
            validate::validate_branch_name(&bucket.branch_name)?;
        }

        // Validate SHAs
        for bucket in &self.buckets {
            for sha in &bucket.commits {
                validate::validate_sha(sha)?;
            }
        }

        Ok(())
    }
}

/// Parse a plan file into a SplitPlan.
///
/// Format:
/// ```text
/// # comments start with #
/// pick <full-sha> <branch-name>
/// pick <full-sha> <branch-name>
/// ```
///
/// Commits are grouped by branch name. The first branch encountered
/// becomes the root of the stack.
pub fn parse_plan_file(path: &Path) -> Result<SplitPlan> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read plan file '{}'", path.display()))?;
    parse_plan(&content)
}

/// Parse plan file content into a SplitPlan.
pub fn parse_plan(content: &str) -> Result<SplitPlan> {
    let mut buckets: Vec<Bucket> = Vec::new();
    let mut bucket_index: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() != 3 {
            bail!(
                "line {}: expected 'pick <sha> <branch-name>', got '{line}'",
                line_num + 1
            );
        }

        let verb = parts[0];
        let sha = parts[1];
        let branch = parts[2];

        if verb != "pick" {
            bail!(
                "line {}: unknown verb '{verb}' (only 'pick' is supported)",
                line_num + 1
            );
        }

        // Validate SHA is full-length hex
        validate::validate_sha(sha)?;
        if sha.len() < 40 {
            bail!(
                "line {}: SHA '{sha}' looks truncated — use full 40-character SHAs",
                line_num + 1
            );
        }

        // Validate branch name
        validate::validate_branch_name(branch)?;

        let sha = sha.to_string();
        let branch = branch.to_string();

        if let Some(&idx) = bucket_index.get(&branch) {
            buckets[idx].commits.push(sha);
        } else {
            let idx = buckets.len();
            bucket_index.insert(branch.clone(), idx);
            buckets.push(Bucket {
                branch_name: branch,
                commits: vec![sha],
            });
        }
    }

    if buckets.is_empty() {
        bail!("Plan file contains no 'pick' lines");
    }

    let plan = SplitPlan { buckets };
    plan.validate()?;
    Ok(plan)
}

pub fn run(args: SplitArgs, ctx: &Ctx) -> Result<()> {
    if args.cont {
        return do_continue(ctx);
    }
    if args.abort {
        return do_abort(ctx);
    }

    // Preconditions
    ctx.require_clean_tree()?;

    let current_branch = ctx.git.current_branch()?;
    if current_branch == "HEAD" {
        bail!("Cannot split from detached HEAD.");
    }

    // Determine base branch and stack context
    let (base_branch, existing_stack) = determine_base(ctx, &current_branch, &args)?;

    // Get commits on the branch
    let commits = ctx.git.log_commits(&base_branch, &current_branch)?;

    if commits.len() < 2 {
        bail!(
            "Branch '{}' has {} commit(s). Split requires at least 2.",
            current_branch,
            commits.len()
        );
    }

    // Check for merge commits
    if let Some(merge) = commits.iter().find(|c| c.parent_count > 1) {
        bail!(
            "Branch contains merge commit {}. Linearize the branch first (e.g., interactive rebase).",
            &merge.full_sha[..12]
        );
    }

    // Get or parse the plan
    let plan = match args.plan {
        Some(ref p) => parse_plan_file(Path::new(p))?,
        None => {
            let existing_branches = ctx.git.all_local_branches()?;
            match super::split_tui::run_split_tui(&commits, &existing_branches)? {
                Some(p) => p,
                None => {
                    ui::info("Split cancelled.");
                    return Ok(());
                }
            }
        }
    };

    // Validate that plan commits match branch commits
    let branch_shas: HashSet<&str> = commits.iter().map(|c| c.full_sha.as_str()).collect();
    let plan_shas: HashSet<&str> = plan
        .buckets
        .iter()
        .flat_map(|b| b.commits.iter().map(|s| s.as_str()))
        .collect();

    if branch_shas != plan_shas {
        let missing_from_plan: Vec<&&str> = branch_shas.difference(&plan_shas).collect();
        let extra_in_plan: Vec<&&str> = plan_shas.difference(&branch_shas).collect();
        let mut msg = String::from("Plan commits don't match branch commits.\n");
        if !missing_from_plan.is_empty() {
            msg.push_str(&format!(
                "  Missing from plan: {}\n",
                missing_from_plan
                    .iter()
                    .map(|s| &s[..12])
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !extra_in_plan.is_empty() {
            msg.push_str(&format!(
                "  Not on branch: {}\n",
                extra_in_plan
                    .iter()
                    .map(|s| &s[..12])
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        bail!("{msg}");
    }

    // Check for branch name collisions
    let existing_branches = ctx.git.all_local_branches()?;
    for bucket in &plan.buckets {
        if existing_branches.contains(&bucket.branch_name) {
            bail!(
                "Branch '{}' already exists. Choose a different name.",
                bucket.branch_name
            );
        }
    }

    // Determine stack name
    let stack_name = match args.name {
        Some(ref n) => {
            validate::validate_stack_name(n)?;
            n.clone()
        }
        None => {
            let derived = current_branch.replace('/', "-");
            validate::validate_stack_name(&derived)?;
            derived
        }
    };

    // If creating a new stack, verify the name doesn't conflict
    if existing_stack.is_none() && ctx.stack_exists(&stack_name) {
        bail!("Stack '{stack_name}' already exists. Use --name to specify a different name.");
    }

    let original_sha = ctx.git.rev_parse("HEAD")?;

    // Build SplitState for crash recovery
    let split_state = SplitState {
        operation: Operation::Split,
        original_branch: current_branch.clone(),
        original_sha: original_sha.clone(),
        base_branch: base_branch.clone(),
        stack_name: stack_name.clone(),
        created_branches: vec![],
        current_bucket_index: 0,
        current_commit_index: 0,
        buckets: plan
            .buckets
            .iter()
            .map(|b| SplitBucket {
                name: b.branch_name.clone(),
                commits: b.commits.clone(),
            })
            .collect(),
    };

    ctx.save_split_state(&split_state)?;

    // Execute the cherry-pick phase
    let result = execute_cherry_picks(ctx, &split_state)?;
    match result {
        CherryPickPhaseResult::Complete { created_branches } => {
            finalize_split(ctx, &current_branch, &base_branch, &stack_name, &created_branches, existing_stack.as_ref())?;
        }
        CherryPickPhaseResult::Conflict { branch } => {
            ui::warn(&format!("Conflict while cherry-picking onto '{branch}'."));
            ui::info("Resolve the conflicts, then run:");
            ui::info("  git add <resolved files>");
            ui::info("  gw split --continue");
            ui::info("");
            ui::info("Or abort the entire split:");
            ui::info("  gw split --abort");
        }
    }

    Ok(())
}

enum CherryPickPhaseResult {
    Complete { created_branches: Vec<String> },
    Conflict { branch: String },
}

/// Execute cherry-picks starting from the given state's position.
fn execute_cherry_picks(ctx: &Ctx, initial_state: &SplitState) -> Result<CherryPickPhaseResult> {
    let mut created_branches = initial_state.created_branches.clone();
    let base = &initial_state.base_branch;

    for bucket_idx in initial_state.current_bucket_index..initial_state.buckets.len() {
        let bucket = &initial_state.buckets[bucket_idx];
        let start_commit_idx = if bucket_idx == initial_state.current_bucket_index {
            initial_state.current_commit_index
        } else {
            0
        };

        // Create the branch if we haven't started this bucket yet
        if start_commit_idx == 0 {
            let start_point = if bucket_idx == 0 {
                base.to_string()
            } else {
                // Base on previous bucket's branch (its current tip)
                initial_state.buckets[bucket_idx - 1].name.clone()
            };
            ctx.git.create_branch(&bucket.name, &start_point)?;
            created_branches.push(bucket.name.clone());

            // Update state with new branch
            let mut state = load_split_state(ctx)?;
            state.created_branches = created_branches.clone();
            state.current_bucket_index = bucket_idx;
            state.current_commit_index = 0;
            ctx.save_split_state(&state)?;
        }

        ctx.git.checkout(&bucket.name)?;

        // Cherry-pick each commit
        for commit_idx in start_commit_idx..bucket.commits.len() {
            let sha = &bucket.commits[commit_idx];

            // Update state BEFORE cherry-pick (crash recovery)
            let mut state = load_split_state(ctx)?;
            state.created_branches = created_branches.clone();
            state.current_bucket_index = bucket_idx;
            state.current_commit_index = commit_idx;
            ctx.save_split_state(&state)?;

            match ctx.git.cherry_pick(sha)? {
                CherryPickResult::Success => {}
                CherryPickResult::Conflict => {
                    return Ok(CherryPickPhaseResult::Conflict {
                        branch: bucket.name.clone(),
                    });
                }
            }
        }
    }

    Ok(CherryPickPhaseResult::Complete { created_branches })
}

/// Load the current SplitState, bailing if not found.
fn load_split_state(ctx: &Ctx) -> Result<SplitState> {
    match ctx.active_state()? {
        Some(ActiveState::Split(s)) => Ok(s),
        _ => bail!("No split in progress."),
    }
}

fn do_continue(ctx: &Ctx) -> Result<()> {
    // Check what kind of state we're in
    match ctx.active_state()? {
        Some(ActiveState::Split(state)) => {
            // We're in the cherry-pick phase
            if ctx.git.has_unresolved_conflicts()? {
                bail!(
                    "There are still unresolved conflicts.\n\
                     Resolve them and run `git add`, then `gw split --continue`."
                );
            }

            // If a cherry-pick is in progress, continue it
            if ctx.git.is_cherry_pick_in_progress() {
                match ctx.git.cherry_pick_continue()? {
                    CherryPickResult::Success => {}
                    CherryPickResult::Conflict => {
                        let bucket = &state.buckets[state.current_bucket_index];
                        ui::warn(&format!(
                            "Still conflicting on '{}'.",
                            bucket.name
                        ));
                        return Ok(());
                    }
                }
            }

            // Advance past the commit we just resolved
            let mut state = state;
            state.current_commit_index += 1;

            // Check if we've finished the current bucket
            let bucket_len = state.buckets[state.current_bucket_index].commits.len();
            if state.current_commit_index >= bucket_len {
                state.current_bucket_index += 1;
                state.current_commit_index = 0;
            }

            // Check if all buckets are done
            if state.current_bucket_index >= state.buckets.len() {
                ctx.save_split_state(&state)?;
                let existing_stack = ctx.find_stack_for_branch(&state.original_branch)?;
                finalize_split(
                    ctx,
                    &state.original_branch.clone(),
                    &state.base_branch.clone(),
                    &state.stack_name.clone(),
                    &state.created_branches.clone(),
                    existing_stack.as_ref(),
                )?;
                return Ok(());
            }

            // Save updated state and continue
            ctx.save_split_state(&state)?;
            let result = execute_cherry_picks(ctx, &state)?;
            match result {
                CherryPickPhaseResult::Complete { created_branches } => {
                    let existing_stack = ctx.find_stack_for_branch(&state.original_branch)?;
                    finalize_split(
                        ctx,
                        &state.original_branch,
                        &state.base_branch,
                        &state.stack_name,
                        &created_branches,
                        existing_stack.as_ref(),
                    )?;
                }
                CherryPickPhaseResult::Conflict { branch } => {
                    ui::warn(&format!("Conflict while cherry-picking onto '{branch}'."));
                    ui::info("Resolve the conflicts, then run:");
                    ui::info("  git add <resolved files>");
                    ui::info("  gw split --continue");
                }
            }
            Ok(())
        }
        Some(ActiveState::Propagation(prop_state)) => {
            // We're in the descendant rebase phase (propagation after cherry-picks completed)
            if prop_state.operation != Operation::Split {
                bail!(
                    "No split in progress. There is an active {} propagation.\n\
                     Use `gw rebase --continue` instead.",
                    match prop_state.operation {
                        Operation::Rebase => "rebase",
                        Operation::Sync => "sync",
                        Operation::Adopt => "adopt",
                        Operation::BranchRemove => "branch remove",
                        Operation::Split => unreachable!(),
                    }
                );
            }
            // Delegate to propagation engine
            match propagation::continue_propagation(ctx)? {
                PropagationResult::Success { rebased_count } => {
                    ui::success(&format!(
                        "Split complete. {rebased_count} descendant branch{} rebased.",
                        if rebased_count == 1 { "" } else { "es" }
                    ));
                }
                PropagationResult::Conflict { branch } => {
                    ui::warn(&format!(
                        "Conflict rebasing descendant '{branch}'."
                    ));
                    ui::info("Resolve conflicts, then run `gw split --continue`.");
                }
            }
            Ok(())
        }
        None => bail!("No split in progress."),
    }
}

fn do_abort(ctx: &Ctx) -> Result<()> {
    match ctx.active_state()? {
        Some(ActiveState::Split(state)) => {
            // Abort any in-progress cherry-pick
            ctx.git.cherry_pick_abort()?;

            // Delete all created branches (idempotent — tolerate "not found")
            for branch in &state.created_branches {
                if ctx.git.branch_exists(branch).unwrap_or(false) {
                    // Can't delete a checked-out branch, so checkout original first
                    let _ = ctx.git.checkout(&state.original_branch);
                    let _ = ctx.git.delete_branch(branch);
                }
            }

            // Return to original branch
            let _ = ctx.git.checkout(&state.original_branch);

            // Clean up state
            ctx.remove_state()?;

            ui::success("Split aborted. All created branches removed.");
            Ok(())
        }
        Some(ActiveState::Propagation(prop_state)) => {
            if prop_state.operation != Operation::Split {
                bail!(
                    "No split in progress. Use `gw rebase --abort` for the active propagation."
                );
            }
            // Abort propagation (restores descendant refs)
            propagation::abort(ctx)?;

            // The propagation abort cleaned up state, but we may also need
            // to clean up the split-created branches and stack config.
            // For simplicity in v1, the propagation abort handles ref restoration,
            // and we inform the user.
            ui::success("Split propagation aborted. Descendant branches restored.");
            Ok(())
        }
        None => bail!("No split in progress."),
    }
}

/// Finalize a successful split: update stack config, rebase descendants, checkout root.
fn finalize_split(
    ctx: &Ctx,
    original_branch: &str,
    base_branch: &str,
    stack_name: &str,
    created_branches: &[String],
    existing_stack: Option<&StackConfig>,
) -> Result<()> {
    let new_branch_entries: Vec<BranchEntry> = created_branches
        .iter()
        .map(|name| BranchEntry { name: name.clone() })
        .collect();

    let new_leaf = created_branches
        .last()
        .context("no branches created")?
        .clone();

    if let Some(stack) = existing_stack {
        // Branch was in an existing stack — replace it with the new branches
        let orig_idx = stack
            .branch_index(original_branch)
            .context("original branch not found in stack")?;

        let mut new_branches = stack.branches.clone();
        // Remove the original branch entry
        new_branches.remove(orig_idx);
        // Insert new branches at the same position
        for (i, entry) in new_branch_entries.iter().enumerate() {
            new_branches.insert(orig_idx + i, entry.clone());
        }

        let updated_stack = StackConfig {
            name: stack.name.clone(),
            base_branch: stack.base_branch.clone(),
            branches: new_branches,
        };
        ctx.save_stack(&updated_stack)?;

        // If there are descendant branches, rebase them onto the new leaf
        let descendants: Vec<String> = stack
            .descendants_of(original_branch)
            .iter()
            .map(|b| b.name.clone())
            .collect();

        if !descendants.is_empty() {
            ui::info(&format!(
                "Rebasing {} descendant branch{} onto '{new_leaf}'...",
                descendants.len(),
                if descendants.len() == 1 { "" } else { "es" }
            ));

            // Build onto targets: first descendant rebases onto new_leaf,
            // rest rebase onto their parent in the updated stack
            let mut onto_targets = vec![new_leaf.clone()];
            for desc in descendants.iter().skip(1) {
                let parent = updated_stack
                    .parent_of(desc)
                    .context("descendant has no parent")?;
                onto_targets.push(parent);
            }

            // Provide upstream overrides so descendants rebase correctly
            // (their old parent was the original branch, not the new leaf)
            let upstream_overrides: Vec<Option<String>> = vec![Some(original_branch.to_string())]
                .into_iter()
                .chain(std::iter::repeat(None).take(descendants.len() - 1))
                .collect();

            match propagation::start_with_upstreams(
                ctx,
                Operation::Split,
                &stack.name,
                &descendants,
                &onto_targets,
                &upstream_overrides,
            )? {
                PropagationResult::Success { rebased_count } => {
                    ui::info(&format!(
                        "{rebased_count} descendant branch{} rebased.",
                        if rebased_count == 1 { "" } else { "es" }
                    ));
                }
                PropagationResult::Conflict { branch } => {
                    ui::warn(&format!(
                        "Conflict rebasing descendant '{branch}'."
                    ));
                    ui::info("Resolve conflicts, then run `gw split --continue`.");
                    return Ok(());
                }
            }
        } else {
            // No descendants — just clean up state
            ctx.remove_state()?;
        }
    } else {
        // Untracked branch — create a new stack
        let config = StackConfig {
            name: stack_name.to_string(),
            base_branch: base_branch.to_string(),
            branches: new_branch_entries,
        };
        ctx.save_stack(&config)?;
        ctx.remove_state()?;
    }

    // Checkout the root of the new stack
    let root = &created_branches[0];
    ctx.git.checkout(root)?;

    ui::success(&format!(
        "Split complete! Created {} branches in stack '{stack_name}':",
        created_branches.len()
    ));
    for (i, name) in created_branches.iter().enumerate() {
        let marker = if i == 0 { " (root)" } else { "" };
        ui::info(&format!("  {name}{marker}"));
    }

    Ok(())
}

/// Determine the base branch for the split.
/// Returns (base_branch, Option<existing_stack>) — if the branch is tracked, we get its stack.
fn determine_base(
    ctx: &Ctx,
    current_branch: &str,
    args: &SplitArgs,
) -> Result<(String, Option<StackConfig>)> {
    // Check if the branch is in an existing stack
    if let Some(stack) = ctx.find_stack_for_branch(current_branch)? {
        let parent = stack
            .parent_of(current_branch)
            .context("branch has no parent in stack")?;
        // --base overrides the stack's parent
        let base = args.base.clone().unwrap_or(parent);
        return Ok((base, Some(stack)));
    }

    // Untracked branch — use --base or infer
    let base = match &args.base {
        Some(b) => {
            validate::validate_branch_name(b)?;
            if !ctx.git.branch_exists(b)? {
                bail!("Base branch '{b}' does not exist.");
            }
            b.clone()
        }
        None => ctx.default_base_branch()?,
    };

    Ok((base, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_plan() {
        let content = "\
# gw split plan
pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa auth
pick bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb auth

pick cccccccccccccccccccccccccccccccccccccccc dashboard
pick dddddddddddddddddddddddddddddddddddddddd dashboard
";
        let plan = parse_plan(content).unwrap();
        assert_eq!(plan.buckets.len(), 2);
        assert_eq!(plan.buckets[0].branch_name, "auth");
        assert_eq!(plan.buckets[0].commits.len(), 2);
        assert_eq!(plan.buckets[1].branch_name, "dashboard");
        assert_eq!(plan.buckets[1].commits.len(), 2);
    }

    #[test]
    fn parse_rejects_short_sha() {
        let content = "pick abc123 my-branch\n";
        let err = parse_plan(content).unwrap_err();
        assert!(err.to_string().contains("truncated"), "{err}");
    }

    #[test]
    fn parse_rejects_single_bucket() {
        let content = "\
pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa only-one
pick bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb only-one
";
        let err = parse_plan(content).unwrap_err();
        assert!(err.to_string().contains("at least 2 buckets"), "{err}");
    }

    #[test]
    fn parse_rejects_empty_plan() {
        let content = "# just a comment\n\n";
        let err = parse_plan(content).unwrap_err();
        assert!(err.to_string().contains("no 'pick' lines"), "{err}");
    }

    #[test]
    fn parse_rejects_unknown_verb() {
        let content = "drop aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa my-branch\n";
        let err = parse_plan(content).unwrap_err();
        assert!(err.to_string().contains("unknown verb"), "{err}");
    }

    #[test]
    fn parse_rejects_malformed_line() {
        let content = "pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
        let err = parse_plan(content).unwrap_err();
        assert!(err.to_string().contains("expected 'pick"), "{err}");
    }

    #[test]
    fn parse_rejects_invalid_branch_name() {
        let content = "pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --bad-branch\n";
        let err = parse_plan(content).unwrap_err();
        assert!(err.to_string().contains("start with '-'"), "{err}");
    }

    #[test]
    fn parse_rejects_duplicate_commits() {
        let content = "\
pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa branch-a
pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa branch-b
";
        let err = parse_plan(content).unwrap_err();
        assert!(err.to_string().contains("multiple buckets"), "{err}");
    }

    #[test]
    fn parse_rejects_duplicate_branch_names() {
        // This can't actually happen with our parser since same-name picks
        // get merged into one bucket. But validate() catches if constructed manually.
        let plan = SplitPlan {
            buckets: vec![
                Bucket {
                    branch_name: "dupe".to_string(),
                    commits: vec!["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()],
                },
                Bucket {
                    branch_name: "dupe".to_string(),
                    commits: vec!["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()],
                },
            ],
        };
        let err = plan.validate().unwrap_err();
        assert!(err.to_string().contains("Duplicate bucket name"), "{err}");
    }

    #[test]
    fn validate_rejects_empty_bucket() {
        let plan = SplitPlan {
            buckets: vec![
                Bucket {
                    branch_name: "a".to_string(),
                    commits: vec!["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()],
                },
                Bucket {
                    branch_name: "b".to_string(),
                    commits: vec![],
                },
            ],
        };
        let err = plan.validate().unwrap_err();
        assert!(err.to_string().contains("no commits assigned"), "{err}");
    }

    #[test]
    fn plan_preserves_bucket_order() {
        let content = "\
pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa third
pick bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb first
pick cccccccccccccccccccccccccccccccccccccccc third
pick dddddddddddddddddddddddddddddddddddddddd second
";
        let plan = parse_plan(content).unwrap();
        assert_eq!(plan.buckets.len(), 3);
        assert_eq!(plan.buckets[0].branch_name, "third");
        assert_eq!(plan.buckets[1].branch_name, "first");
        assert_eq!(plan.buckets[2].branch_name, "second");
    }
}

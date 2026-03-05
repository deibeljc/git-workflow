use anyhow::{bail, Result};

use crate::cli::StackCommands;
use crate::context::Ctx;
use crate::state::{BranchEntry, StackConfig};
use crate::validate;
use crate::ui;

pub fn run(cmd: StackCommands, ctx: &Ctx) -> Result<()> {
    match cmd {
        StackCommands::Create { name, base } => create(ctx, &name, base.as_deref()),
        StackCommands::Delete { name } => delete(ctx, &name),
        StackCommands::List => list(ctx),
    }
}

fn create(ctx: &Ctx, name: &str, base: Option<&str>) -> Result<()> {
    validate::validate_stack_name(name)?;
    validate::validate_branch_name(name)?; // name is also used as branch name

    if ctx.stack_exists(name) {
        bail!("Stack '{name}' already exists. Use `gw stack delete {name}` first.");
    }

    if ctx.git.branch_exists(name)? {
        bail!(
            "Branch '{name}' already exists. Use `gw adopt {name}` to track it, or choose a different name."
        );
    }

    let base_branch = match base {
        Some(b) => b.to_string(),
        None => ctx.default_base_branch()?,
    };

    let head = ctx.git.rev_parse(&format!("refs/heads/{base_branch}"))?;
    ctx.git.create_branch(name, &head)?;

    let config = StackConfig {
        name: name.to_string(),
        base_branch,
        branches: vec![BranchEntry {
            name: name.to_string(),
        }],
    };

    ctx.save_stack(&config)?;
    ctx.git.checkout(name)?;

    let short_sha = &head[..7.min(head.len())];
    ui::success(&format!(
        "Created stack '{}' off {} @ {}",
        config.name, config.base_branch, short_sha
    ));
    ui::info(&format!("Created and checked out branch '{name}' (root)"));

    Ok(())
}

fn delete(ctx: &Ctx, name: &str) -> Result<()> {
    if !ctx.stack_exists(name) {
        bail!("Stack '{name}' does not exist.");
    }

    let config = ctx.load_stack(name)?;
    ctx.delete_stack(name)?;

    let branch_names: Vec<&str> = config.branches.iter().map(|b| b.name.as_str()).collect();
    ui::success(&format!("Untracked stack '{name}'"));
    ui::info(&format!(
        "Branches remain (no longer managed by gw): {}",
        branch_names.join(", ")
    ));

    Ok(())
}

fn list(ctx: &Ctx) -> Result<()> {
    let stacks = ctx.load_all_stacks()?;

    if stacks.is_empty() {
        ui::info("No stacks. Create one with `gw stack create <name>`.");
        return Ok(());
    }

    for stack in &stacks {
        let branch_count = stack.branches.len();
        let branches_word = if branch_count == 1 {
            "branch"
        } else {
            "branches"
        };
        println!(
            "{} ({} {}, base: {})",
            stack.name, branch_count, branches_word, stack.base_branch
        );
    }

    Ok(())
}

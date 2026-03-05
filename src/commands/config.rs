use anyhow::{bail, Result};

use crate::cli::ConfigCommands;
use crate::context::Ctx;
use crate::ui;
use crate::validate;

pub fn run(cmd: ConfigCommands, ctx: &Ctx) -> Result<()> {
    match cmd {
        ConfigCommands::SetBase { branch } => set_base(ctx, &branch),
        ConfigCommands::Show => show(ctx),
    }
}

fn set_base(ctx: &Ctx, branch: &str) -> Result<()> {
    validate::validate_branch_name(branch)?;

    if !ctx.git.branch_exists(branch)? {
        bail!("Branch '{branch}' does not exist locally.");
    }

    let mut config = ctx.load_config()?;
    config.default_base = Some(branch.to_string());
    ctx.save_config(&config)?;

    ui::success(&format!("Default base branch set to '{branch}'."));
    ui::info("New stacks will use this as the base unless --base is specified.");

    Ok(())
}

fn show(ctx: &Ctx) -> Result<()> {
    let config = ctx.load_config()?;

    println!("{}", "gw config".bold());
    println!();

    match config.default_base {
        Some(ref base) => {
            println!("  default base branch: {}", base.cyan());
        }
        None => {
            let inferred = ctx.default_base_branch()?;
            println!(
                "  default base branch: {} {}",
                inferred.cyan(),
                "(auto-detected)".dimmed()
            );
        }
    }

    Ok(())
}

use colored::Colorize;

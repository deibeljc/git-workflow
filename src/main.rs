use anyhow::{bail, Result};
use clap::Parser;

use gw::cli::{Cli, Commands};
use gw::context::Ctx;
use gw::{commands, state};

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Default to `gw tree` when no subcommand is given
    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            let ctx = Ctx::discover()?;
            return commands::tree::run(&ctx, false);
        }
    };

    // These don't need a Ctx
    if let Commands::Completions(args) = &command {
        return commands::completions::run(&args.shell);
    }
    if let Commands::McpSetup = &command {
        return commands::mcp_setup::run();
    }
    if let Commands::McpServer = &command {
        return commands::mcp_server::run();
    }

    let ctx = Ctx::discover()?;

    // State guard: block most commands if a propagation is in progress
    if let Some(ref prop_state) = ctx.propagation_state()? {
        let allowed = matches!(
            &command,
            Commands::Rebase(args) if args.cont || args.abort
        ) || matches!(&command, Commands::Log(_))
            || matches!(&command, Commands::Switch(_));

        if !allowed {
            let op = match prop_state.operation {
                state::Operation::Rebase => "rebase",
                state::Operation::Sync => "sync",
                state::Operation::Adopt => "adopt",
                state::Operation::BranchRemove => "branch remove",
            };
            bail!(
                "A {op} propagation is in progress on stack '{}'.\n\
                 Run `gw rebase --continue` or `gw rebase --abort` first.",
                prop_state.stack
            );
        }
    }

    match command {
        Commands::Stack(args) => commands::stack::run(args.command, &ctx),
        Commands::Branch(args) => commands::branch::run(args.command, &ctx),
        Commands::Adopt(args) => commands::adopt::run(args, &ctx),
        Commands::Rebase(args) => commands::rebase::run(args, &ctx),
        Commands::Sync(args) => commands::sync::run(args, &ctx),
        Commands::Push(args) => commands::push::run(args, &ctx),
        Commands::Switch(args) => commands::switch::run(args.branch, &ctx),
        Commands::Log(args) => commands::tree::run(&ctx, args.pr),
        Commands::Config(args) => commands::config::run(args.command, &ctx),
        Commands::Completions(_) | Commands::McpSetup | Commands::McpServer => unreachable!(),
    }
}

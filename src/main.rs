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

    // State guard: block most commands if a propagation or split is in progress
    if let Some(ref active) = ctx.active_state()? {
        match active {
            state::ActiveState::Propagation(prop_state) => {
                let is_split_op = prop_state.operation == state::Operation::Split;
                let allowed = matches!(
                    &command,
                    Commands::Rebase(args) if args.cont || args.abort
                ) || matches!(
                    &command,
                    Commands::Split(args) if (args.cont || args.abort) && is_split_op
                ) || matches!(&command, Commands::Log(_))
                    || matches!(&command, Commands::Switch(_))
                    || matches!(&command, Commands::Status)
                    || matches!(&command, Commands::Diff(_));

                if !allowed {
                    if is_split_op {
                        bail!(
                            "A split propagation is in progress on stack '{}'.\n\
                             Run `gw split --continue` or `gw split --abort` first.",
                            prop_state.stack
                        );
                    }
                    let op = match prop_state.operation {
                        state::Operation::Rebase => "rebase",
                        state::Operation::Sync => "sync",
                        state::Operation::Adopt => "adopt",
                        state::Operation::BranchRemove => "branch remove",
                        state::Operation::Split => unreachable!(),
                    };
                    bail!(
                        "A {op} propagation is in progress on stack '{}'.\n\
                         Run `gw rebase --continue` or `gw rebase --abort` first.",
                        prop_state.stack
                    );
                }
            }
            state::ActiveState::Split(split_state) => {
                let allowed = matches!(
                    &command,
                    Commands::Split(args) if args.cont || args.abort
                ) || matches!(&command, Commands::Log(_))
                    || matches!(&command, Commands::Switch(_))
                    || matches!(&command, Commands::Status)
                    || matches!(&command, Commands::Diff(_));

                if !allowed {
                    bail!(
                        "A split is in progress on branch '{}'.\n\
                         Run `gw split --continue` or `gw split --abort` first.",
                        split_state.original_branch
                    );
                }
            }
        }
    }

    match command {
        Commands::Status => commands::status::run(&ctx),
        Commands::Diff(args) => commands::diff::run(&ctx, args.stat, args.no_difftastic),
        Commands::Stack(args) => commands::stack::run(args.command, &ctx),
        Commands::Branch(args) => commands::branch::run(args.command, &ctx),
        Commands::Adopt(args) => commands::adopt::run(args, &ctx),
        Commands::Rebase(args) => commands::rebase::run(args, &ctx),
        Commands::Sync(args) => commands::sync::run(args, &ctx),
        Commands::Push(args) => commands::push::run(args, &ctx),
        Commands::Switch(args) => commands::switch::run(args.branch, &ctx),
        Commands::Log(args) => commands::tree::run(&ctx, args.pr),
        Commands::Split(args) => commands::split::run(args, &ctx),
        Commands::Config(args) => commands::config::run(args.command, &ctx),
        Commands::Completions(_) | Commands::McpSetup | Commands::McpServer => unreachable!(),
    }
}

use anyhow::Result;
use clap::Parser;

use gw::cli::{Cli, Commands};
use gw::context::Ctx;
use gw::{commands, state, ui};

fn main() -> Result<()> {
    let cli = Cli::parse();

    let ctx = Ctx::discover()?;

    // State guard: block most commands if a propagation is in progress
    if let Some(ref prop_state) = ctx.propagation_state()? {
        let allowed = matches!(
            &cli.command,
            Commands::Rebase(args) if args.cont || args.abort
        ) || matches!(&cli.command, Commands::Tree);

        if !allowed {
            ui::error(&format!(
                "A {} propagation is in progress on stack '{}'.",
                match prop_state.operation {
                    state::Operation::Rebase => "rebase",
                    state::Operation::Sync => "sync",
                    state::Operation::Adopt => "adopt",
                    state::Operation::BranchRemove => "branch remove",
                },
                prop_state.stack
            ));
            ui::info("Run `gw rebase --continue` or `gw rebase --abort` first.");
            std::process::exit(1);
        }
    }

    match cli.command {
        Commands::Stack(args) => commands::stack::run(args.command, &ctx),
        Commands::Branch(args) => commands::branch::run(args.command, &ctx),
        Commands::Adopt(args) => commands::adopt::run(args, &ctx),
        Commands::Rebase(args) => commands::rebase::run(args, &ctx),
        Commands::Sync(args) => commands::sync::run(args, &ctx),
        Commands::Push(args) => commands::push::run(args, &ctx),
        Commands::Tree => commands::tree::run(&ctx),
        Commands::Config(args) => commands::config::run(args.command, &ctx),
    }
}

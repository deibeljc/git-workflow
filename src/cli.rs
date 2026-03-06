use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gw", version, about = "Git stacked branch manager")]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage stacks
    Stack(StackArgs),
    /// Manage branches within a stack
    Branch(BranchArgs),
    /// Adopt existing branches into a stack
    Adopt(AdoptArgs),
    /// Propagate rebases to descendant branches
    Rebase(RebaseArgs),
    /// Sync stacks with the base branch
    Sync(SyncArgs),
    /// Push the current branch
    Push(PushArgs),
    /// Switch to a branch tracked by gw
    Switch(SwitchArgs),
    /// Show log of all stacks with branches and commits
    #[command(alias = "tree")]
    Log(TreeArgs),
    /// Configure gw settings
    Config(ConfigArgs),
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Set up the MCP server for Claude Code
    #[command(name = "mcp-setup")]
    McpSetup,
    /// Run the MCP server (used by Claude Code, not for direct use)
    #[command(name = "mcp-server", hide = true)]
    McpServer,
}

// -- Stack subcommands --

#[derive(Args)]
#[command(flatten_help = true)]
pub struct StackArgs {
    #[command(subcommand)]
    pub command: StackCommands,
}

#[derive(Subcommand)]
pub enum StackCommands {
    /// Create a new stack
    Create {
        /// Name for the stack (also creates a branch with this name)
        name: String,
        /// Base branch to stack off of (defaults to current branch)
        #[arg(long)]
        base: Option<String>,
    },
    /// Delete a stack (branches are NOT deleted)
    Delete {
        /// Stack name to delete
        name: String,
    },
    /// List all stacks
    List,
}

// -- Branch subcommands --

#[derive(Args)]
#[command(flatten_help = true)]
pub struct BranchArgs {
    #[command(subcommand)]
    pub command: BranchCommands,
}

#[derive(Subcommand)]
pub enum BranchCommands {
    /// Create a new branch in the current stack
    Create {
        /// Branch name
        name: String,
    },
    /// Remove a branch from its stack (git branch is NOT deleted)
    Remove {
        /// Branch name to remove
        name: String,
    },
}

// -- Adopt --

#[derive(Args)]
pub struct AdoptArgs {
    /// Branch names in stack order (first = root, last = leaf)
    #[arg(required = true)]
    pub branches: Vec<String>,
    /// Base branch (inferred from first branch's parent if not specified)
    #[arg(long)]
    pub base: Option<String>,
    /// Stack name (defaults to first branch name)
    #[arg(long)]
    pub name: Option<String>,
    /// Skip confirmation prompt
    #[arg(long)]
    pub yes: bool,
}

// -- Rebase --

#[derive(Args)]
pub struct RebaseArgs {
    /// Continue after resolving conflicts
    #[arg(long = "continue", id = "continue", conflicts_with = "abort")]
    pub cont: bool,
    /// Abort and roll back all branches
    #[arg(long, conflicts_with = "continue")]
    pub abort: bool,
}

// -- Sync --

#[derive(Args)]
pub struct SyncArgs {
    /// Only sync a specific stack
    #[arg(long)]
    pub stack: Option<String>,
    /// Manually indicate a branch was merged (when gh is unavailable)
    #[arg(long)]
    pub merged: Option<String>,
    /// Rebase the entire stack onto the latest base branch
    #[arg(long)]
    pub rebase: bool,
}

// -- Push --

#[derive(Args)]
pub struct PushArgs {
    /// Skip confirmation prompt for force push
    #[arg(long)]
    pub yes: bool,
}

// -- Switch --

#[derive(Args)]
pub struct SwitchArgs {
    /// Branch name to switch to (interactive picker if omitted)
    pub branch: Option<String>,
}

// -- Tree --

#[derive(Args)]
pub struct TreeArgs {
    /// Show PR status from GitHub (requires gh CLI, adds latency)
    #[arg(long)]
    pub pr: bool,
}

// -- Config --

#[derive(Args)]
#[command(flatten_help = true)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Set the default base branch (e.g., dev, main, master)
    #[command(name = "set-base")]
    SetBase {
        /// Branch name to use as the default base
        branch: String,
    },
    /// Show current configuration
    Show,
}

// -- Completions --

#[derive(Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_parser = ["zsh", "bash", "fish"])]
    pub shell: String,
}

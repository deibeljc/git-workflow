use std::process::Command;

use rmcp::{
    Error as McpError, ServerHandler,
    model::{ServerCapabilities, ServerInfo},
    tool,
    service::ServiceExt,
};

fn gw(args: &[&str]) -> Result<String, McpError> {
    let output = Command::new("gw")
        .args(args)
        .output()
        .map_err(|e| McpError::internal_error(format!("failed to run gw: {e}"), None))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(McpError::internal_error(
            format!("gw {} failed: {}", args.join(" "), if stderr.is_empty() { &stdout } else { &stderr }),
            None,
        ));
    }

    Ok(if stdout.is_empty() { stderr } else { stdout })
}

#[derive(Debug, Clone, Default)]
struct GwServer;

#[tool(tool_box)]
impl GwServer {
    #[tool(description = "Show all stacks with branches and commits")]
    fn gw_log(&self) -> String {
        gw(&["log"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Show all stacks with branches, commits, and PR status from GitHub")]
    fn gw_log_pr(&self) -> String {
        gw(&["log", "--pr"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Create a new stack off the base branch")]
    fn gw_stack_create(
        &self,
        #[tool(param)]
        #[schemars(description = "Name for the stack")]
        name: String,
    ) -> String {
        gw(&["stack", "create", &name]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Delete a stack (branches are NOT deleted)")]
    fn gw_stack_delete(
        &self,
        #[tool(param)]
        #[schemars(description = "Stack name to delete")]
        name: String,
    ) -> String {
        gw(&["stack", "delete", &name]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "List all stacks")]
    fn gw_stack_list(&self) -> String {
        gw(&["stack", "list"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Add a new branch to the current stack (must be on the leaf branch)")]
    fn gw_branch_create(
        &self,
        #[tool(param)]
        #[schemars(description = "Branch name")]
        name: String,
    ) -> String {
        gw(&["branch", "create", &name]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Remove a branch from its stack (git branch is NOT deleted)")]
    fn gw_branch_remove(
        &self,
        #[tool(param)]
        #[schemars(description = "Branch name to remove")]
        name: String,
    ) -> String {
        gw(&["branch", "remove", &name]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Switch to a branch tracked by gw")]
    fn gw_switch(
        &self,
        #[tool(param)]
        #[schemars(description = "Branch name to switch to")]
        branch: String,
    ) -> String {
        gw(&["switch", &branch]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Propagate rebases to all descendant branches in the current stack")]
    fn gw_rebase(&self) -> String {
        gw(&["rebase"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Continue a rebase after resolving conflicts")]
    fn gw_rebase_continue(&self) -> String {
        gw(&["rebase", "--continue"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Abort a rebase and roll back all branches")]
    fn gw_rebase_abort(&self) -> String {
        gw(&["rebase", "--abort"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Fetch base branch, detect squash merges, and rebase remaining stack")]
    fn gw_sync(&self) -> String {
        gw(&["sync"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Sync and explicitly rebase the entire stack onto the latest base branch")]
    fn gw_sync_rebase(&self) -> String {
        gw(&["sync", "--rebase"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Manually indicate a branch was merged, then sync")]
    fn gw_sync_merged(
        &self,
        #[tool(param)]
        #[schemars(description = "Branch name that was merged")]
        branch: String,
    ) -> String {
        gw(&["sync", "--merged", &branch]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Push the current branch. Will fail if the branch has diverged and needs a force push. Use gw_force_push if the user confirms they want to force push.")]
    fn gw_push(&self) -> String {
        gw(&["push"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Force push the current branch with lease. Only use this after the user has explicitly confirmed they want to force push.")]
    fn gw_force_push(&self) -> String {
        gw(&["push", "--yes"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Split the current branch into a stack of focused branches using a plan file. The plan is a series of 'pick <full-40-char-sha> <branch-name>' lines. Commits are grouped by branch name, and each unique branch name becomes a branch in the new stack. Requires at least 2 branches. Before calling this, use git log to get the full SHAs of the commits on the current branch.")]
    fn gw_split(
        &self,
        #[tool(param)]
        #[schemars(description = "Plan file content: lines of 'pick <full-sha> <branch-name>'. Example:\npick abc123...def456 feature-models\npick 789abc...012def feature-api")]
        plan: String,
        #[tool(param)]
        #[schemars(description = "Optional stack name (defaults to current branch name)")]
        name: Option<String>,
    ) -> String {
        // Write plan to a temp file
        let tmp = std::env::temp_dir().join(format!("gw-split-{}.txt", std::process::id()));
        if let Err(e) = std::fs::write(&tmp, &plan) {
            return format!("Failed to write plan file: {e}");
        }

        let tmp_str = tmp.to_string_lossy().to_string();
        let mut args = vec!["split", "--plan", &tmp_str, "--yes"];
        let name_val;
        if let Some(ref n) = name {
            name_val = n.clone();
            args.push("--name");
            args.push(&name_val);
        }

        let result = gw(&args).unwrap_or_else(|e| format!("{e}"));
        let _ = std::fs::remove_file(&tmp);
        result
    }

    #[tool(description = "Continue a split after resolving cherry-pick conflicts. Resolve conflicts, git add the files, then call this.")]
    fn gw_split_continue(&self) -> String {
        gw(&["split", "--continue"]).unwrap_or_else(|e| format!("{e}"))
    }

    #[tool(description = "Abort a split in progress and roll back all created branches")]
    fn gw_split_abort(&self) -> String {
        gw(&["split", "--abort"]).unwrap_or_else(|e| format!("{e}"))
    }
}

#[tool(tool_box)]
impl ServerHandler for GwServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("gw is a git stacked branch manager. Use these tools to create stacks, manage branches, propagate rebases, and sync with merged PRs.".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tokio::main]
pub async fn run() -> anyhow::Result<()> {
    let server = GwServer;
    let service = server
        .serve(rmcp::transport::io::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

use anyhow::Result;
use colored::Colorize;
use std::collections::{HashMap, HashSet};

use crate::context::Ctx;
use crate::ui;

pub fn run(ctx: &Ctx) -> Result<()> {
    let stacks = ctx.load_all_stacks()?;

    if stacks.is_empty() {
        ui::info("No stacks. Create one with `gw stack create <name>`.");
        return Ok(());
    }

    let current_branch = ctx.git.current_branch().unwrap_or_default();
    let all_branches = ctx.git.all_local_branches().unwrap_or_default();

    // Batch PR status if gh is available
    let pr_status = batch_pr_status();

    for (i, stack) in stacks.iter().enumerate() {
        if i > 0 {
            println!();
        }

        // Print base branch
        println!("{}", stack.base_branch.dimmed());

        let branch_count = stack.branches.len();
        for (idx, branch) in stack.branches.iter().enumerate() {
            let is_last = idx == branch_count - 1;
            let is_current = branch.name == current_branch;
            let is_root = idx == 0;
            let exists = all_branches.contains(&branch.name);

            // Tree connector
            let connector = if is_last { "└── " } else { "├── " };

            // Build the branch display
            let mut parts: Vec<String> = vec![];

            // Branch name (highlighted if current)
            if is_current {
                parts.push(format!("{}", branch.name.green().bold()));
            } else if !exists {
                parts.push(format!("{}", branch.name.red().strikethrough()));
            } else {
                parts.push(branch.name.clone());
            }

            // Role markers
            if is_root {
                parts.push("(root)".dimmed().to_string());
            }
            if is_current {
                parts.push("*".green().bold().to_string());
            }

            // PR status
            if let Some(status) = pr_status.get(&branch.name) {
                parts.push(format!("← {}", status.cyan()));
            }

            // Commit count ahead of parent
            if exists {
                let parent = stack.parent_of(&branch.name).unwrap_or_default();
                if let Ok(count) = ctx.git.commit_count_between(&parent, &branch.name) {
                    if count > 0 {
                        let label = format!(
                            "{count} commit{} ahead",
                            if count == 1 { "" } else { "s" }
                        );
                        parts.push(label.dimmed().to_string());
                    }
                }
            }

            // Remote status
            if exists {
                if let Ok(diverged) = ctx.git.has_diverged_from_remote(&branch.name) {
                    if diverged {
                        parts.push("[diverged]".yellow().to_string());
                    }
                }
                // Check if ahead of remote (needs push)
                if let Ok(needs_push) = check_needs_push(ctx, &branch.name) {
                    if needs_push {
                        parts.push("[needs push]".yellow().to_string());
                    }
                }
            }

            if !exists {
                parts.push("[missing]".red().to_string());
            }

            // Print with indentation based on position
            let indent = "    ".repeat(idx);
            println!("{indent}{connector}{}", parts.join(" "));
        }
    }

    Ok(())
}

/// Check if local branch is ahead of remote (needs push).
fn check_needs_push(ctx: &Ctx, branch: &str) -> Result<bool> {
    let remote_ref = format!("origin/{branch}");
    let remote_exists = ctx
        .git
        .run(&[
            "rev-parse",
            "--verify",
            &format!("refs/remotes/{remote_ref}"),
        ])
        .is_ok();

    if !remote_exists {
        return Ok(false);
    }

    let local_sha = ctx.git.rev_parse(&format!("refs/heads/{branch}"))?;
    let remote_sha = ctx.git.rev_parse(&format!("refs/remotes/{remote_ref}"))?;

    if local_sha == remote_sha {
        return Ok(false);
    }

    // Check if local is ahead (remote is ancestor of local)
    ctx.git.is_ancestor(&remote_sha, &local_sha)
}

/// Batch-fetch PR status via a single gh CLI call.
/// Returns a map of branch_name -> PR status string.
fn batch_pr_status() -> HashMap<String, String> {
    let mut result = HashMap::new();

    let output = std::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--json",
            "headRefName,number,state",
            "--limit",
            "100",
        ])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return result, // gh not available, return empty
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Simple JSON parsing without pulling in serde_json
    // Format: [{"headRefName":"branch","number":42,"state":"OPEN"}, ...]
    for line in stdout.split('{') {
        if let (Some(branch), Some(number), Some(state)) = (
            extract_json_string(line, "headRefName"),
            extract_json_number(line, "number"),
            extract_json_string(line, "state"),
        ) {
            let status = match state.as_str() {
                "OPEN" => format!("PR #{number} open"),
                "CLOSED" => format!("PR #{number} closed"),
                "MERGED" => format!("PR #{number} merged"),
                _ => format!("PR #{number}"),
            };
            result.insert(branch, status);
        }
    }

    result
}

fn extract_json_string(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = text.find(&pattern)? + pattern.len();
    let end = text[start..].find('"')? + start;
    Some(text[start..end].to_string())
}

fn extract_json_number(text: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{key}\":");
    let start = text.find(&pattern)? + pattern.len();
    let num_str: String = text[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

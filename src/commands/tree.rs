use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

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

        let branch_count = stack.branches.len();

        for (idx, branch) in stack.branches.iter().enumerate() {
            let is_last_branch = idx == branch_count - 1;
            let is_current = branch.name == current_branch;
            let is_root = idx == 0;
            let exists = all_branches.contains(&branch.name);

            // === Branch line ===
            let connector = if is_root {
                // Root connects from base
                format!("{} → ", stack.base_branch.dimmed())
            } else {
                "  → ".to_string()
            };

            let mut name_display = if is_current {
                format!("{}", branch.name.green().bold())
            } else if !exists {
                format!("{}", branch.name.red().strikethrough())
            } else {
                format!("{}", branch.name.white().bold())
            };

            if is_current {
                name_display = format!("{name_display} {}", "●".green());
            }

            // Annotations after the name
            let mut annotations: Vec<String> = vec![];

            if is_root {
                annotations.push("root".dimmed().to_string());
            }

            if let Some(status) = pr_status.get(&branch.name) {
                annotations.push(status.cyan().to_string());
            }

            if exists {
                if let Ok(diverged) = ctx.git.has_diverged_from_remote(&branch.name) {
                    if diverged {
                        annotations.push("diverged".yellow().to_string());
                    }
                }
                if let Ok(true) = check_needs_push(ctx, &branch.name) {
                    annotations.push("needs push".yellow().to_string());
                }
            }

            if !exists {
                annotations.push("missing".red().to_string());
            }

            let suffix = if annotations.is_empty() {
                String::new()
            } else {
                format!(" {}", annotations.join(" · "))
            };

            println!("{connector}{name_display}{suffix}");

            // === Commit lines (sub-items of the branch) ===
            if exists {
                let parent = stack.parent_of(&branch.name).unwrap_or_default();
                if let Ok(commits) = ctx.git.log_oneline(&parent, &branch.name, 10) {
                    let commit_count = commits.len();
                    let pipe = if is_last_branch { " " } else { "│" };

                    for (ci, (sha, subject)) in commits.iter().enumerate() {
                        let is_last_commit = ci == commit_count - 1;
                        let commit_connector = if is_last_commit { "╰─" } else { "├─" };

                        // Indent to align under the branch name
                        let indent = if is_root {
                            // Account for "base → " prefix width
                            " ".repeat(stack.base_branch.len() + 3)
                        } else {
                            "    ".to_string()
                        };

                        println!(
                            "{indent}{pipe} {commit_connector} {} {}",
                            sha.yellow(),
                            subject.dimmed()
                        );
                    }

                    // Spacing line between branches (if not last)
                    if !is_last_branch && commit_count > 0 {
                        let indent = if is_root {
                            " ".repeat(stack.base_branch.len() + 3)
                        } else {
                            "    ".to_string()
                        };
                        println!("{indent}│");
                    }
                }
            }
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

    ctx.git.is_ancestor(&remote_sha, &local_sha)
}

/// Batch-fetch PR status via a single gh CLI call.
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
        _ => return result,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

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
    let num_str: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num_str.parse().ok()
}

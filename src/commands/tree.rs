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
    let pr_status = batch_pr_status();

    // Group stacks by base branch (preserve order)
    let mut by_base: Vec<(String, Vec<usize>)> = vec![];
    for (i, stack) in stacks.iter().enumerate() {
        if let Some(entry) = by_base.iter_mut().find(|(b, _)| *b == stack.base_branch) {
            entry.1.push(i);
        } else {
            by_base.push((stack.base_branch.clone(), vec![i]));
        }
    }

    let mut first_output = true;
    for (base, stack_indices) in &by_base {
        for &stack_idx in stack_indices {
            let stack = &stacks[stack_idx];
            let branch_count = stack.branches.len();
            if branch_count == 0 {
                continue;
            }

            if !first_output {
                println!();
            }
            first_output = false;

            // Stack header: base branch
            println!("{}  {}", "◇".cyan(), base.cyan().bold());

            for (idx, branch) in stack.branches.iter().enumerate() {
                let is_last = idx == branch_count - 1;
                let is_current = branch.name == current_branch;
                let is_root = idx == 0;
                let exists = all_branches.contains(&branch.name);

                let marker = if is_current {
                    "@".green().bold().to_string()
                } else {
                    "◆".blue().to_string()
                };

                let name_str = if is_current {
                    branch.name.green().bold().to_string()
                } else if !entry_exists(exists) {
                    branch.name.red().strikethrough().to_string()
                } else {
                    branch.name.white().bold().to_string()
                };

                // Tags
                let mut tags: Vec<String> = vec![];
                if is_root {
                    tags.push("root".blue().dimmed().to_string());
                }
                if let Some(status) = pr_status.get(&branch.name) {
                    tags.push(status.magenta().to_string());
                }
                if exists {
                    if let Ok(diverged) = ctx.git.has_diverged_from_remote(&branch.name) {
                        if diverged {
                            tags.push("diverged".yellow().to_string());
                        }
                    }
                    if let Ok(true) = check_needs_push(ctx, &branch.name) {
                        tags.push("needs push".yellow().to_string());
                    }
                }
                if !exists {
                    tags.push("missing".red().to_string());
                }
                let tag_str = if tags.is_empty() {
                    String::new()
                } else {
                    format!("  {}", tags.join("  "))
                };

                let fork = if is_last { "╰─" } else { "├─" };
                let pipe = if is_last { " " } else { "│" };

                println!("{}  {marker}  {name_str}{tag_str}", fork.dimmed());

                // Commits
                if exists {
                    let parent_name = stack.parent_of(&branch.name).unwrap_or_default();
                    if let Ok(commits) =
                        ctx.git.log_oneline(&parent_name, &branch.name, 10)
                    {
                        for (sha, subject) in &commits {
                            println!(
                                "{}   {} {} {}",
                                pipe.dimmed(),
                                "│".dimmed(),
                                sha.yellow(),
                                subject.dimmed()
                            );
                        }
                    }
                }

                // Spacing between branches within the stack
                if !is_last {
                    println!("{}", pipe.dimmed());
                }
            }
        }
    }

    Ok(())
}

fn entry_exists(exists: bool) -> bool {
    exists
}

fn check_needs_push(ctx: &Ctx, branch: &str) -> Result<bool> {
    let remote_ref = format!("origin/{branch}");
    let remote_exists = ctx
        .git
        .run(&["rev-parse", "--verify", &format!("refs/remotes/{remote_ref}")])
        .is_ok();
    if !remote_exists { return Ok(false); }
    let local_sha = ctx.git.rev_parse(&format!("refs/heads/{branch}"))?;
    let remote_sha = ctx.git.rev_parse(&format!("refs/remotes/{remote_ref}"))?;
    if local_sha == remote_sha { return Ok(false); }
    ctx.git.is_ancestor(&remote_sha, &local_sha)
}

fn batch_pr_status() -> HashMap<String, String> {
    let mut result = HashMap::new();
    let output = std::process::Command::new("gh")
        .args(["pr", "list", "--state", "all", "--json", "headRefName,number,state", "--limit", "100"])
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
    let num_str: String = text[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

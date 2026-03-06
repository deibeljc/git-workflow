use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use crate::context::Ctx;
use crate::gh;
use crate::ui;

pub fn run(ctx: &Ctx, show_pr: bool) -> Result<()> {
    let stacks = ctx.load_all_stacks()?;

    if stacks.is_empty() {
        ui::info("No stacks. Create one with `gw stack create <name>`.");
        return Ok(());
    }

    let current_branch = ctx.git.current_branch().unwrap_or_default();
    let ref_info = batch_ref_info(ctx);

    // Collect commits for all branches
    let mut commit_cache: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for stack in &stacks {
        for branch in &stack.branches {
            if ref_info.contains_key(&branch.name) {
                let parent = stack.parent_of(&branch.name).unwrap_or_default();
                if let Ok(commits) = ctx.git.log_oneline(&parent, &branch.name, 10) {
                    commit_cache.insert(branch.name.clone(), commits);
                }
            }
        }
    }

    // Detect which branches need a rebase (parent tip has moved past the fork point)
    let mut needs_rebase: HashMap<String, bool> = HashMap::new();
    for stack in &stacks {
        for (idx, branch) in stack.branches.iter().enumerate() {
            if idx == 0 {
                continue; // root can't need a rebase within its stack
            }
            if !ref_info.contains_key(&branch.name) {
                continue; // missing branch
            }
            let parent_name = stack.parent_of(&branch.name).unwrap_or_default();
            if let Ok(parent_sha) = ctx.git.rev_parse(&parent_name) {
                if let Ok(mb) = ctx.git.merge_base(&branch.name, &parent_name) {
                    if mb != parent_sha {
                        needs_rebase.insert(branch.name.clone(), true);
                    }
                }
            }
        }
    }

    // Group stacks by base branch
    let mut by_base: Vec<(String, Vec<usize>)> = vec![];
    for (i, stack) in stacks.iter().enumerate() {
        if let Some(entry) = by_base.iter_mut().find(|(b, _)| *b == stack.base_branch) {
            entry.1.push(i);
        } else {
            by_base.push((stack.base_branch.clone(), vec![i]));
        }
    }

    // Compute how far behind the base branch tip each stack's root is
    let mut behind_counts: HashMap<String, usize> = HashMap::new();
    for stack in &stacks {
        if let Some(root) = stack.branches.first() {
            if let Ok(mb) = ctx.git.merge_base(&root.name, &stack.base_branch) {
                let base_sha = ctx.git.rev_parse(&stack.base_branch).unwrap_or_default();
                if mb != base_sha {
                    if let Ok(count) = ctx.git.run(&[
                        "rev-list", "--count", &format!("{mb}..{}", stack.base_branch),
                    ]) {
                        if let Ok(n) = count.parse::<usize>() {
                            if n > 0 {
                                behind_counts.insert(stack.name.clone(), n);
                            }
                        }
                    }
                }
            }
        }
    }

    // PR status (only when requested)
    let pr_status = if show_pr {
        gh::batch_pr_status()
    } else {
        HashMap::new()
    };

    // === Render ===
    // Track total lines + branch positions for PR retroactive update
    let mut total_lines = 0;
    let mut branch_positions: HashMap<String, usize> = HashMap::new();

    for (base, stack_indices) in &by_base {
        // Base branch header (top-level root)
        let base_sha = ctx.git.rev_parse_short(base).unwrap_or_default();

        // Check if any stacks are behind
        let max_behind = stack_indices
            .iter()
            .filter_map(|&si| behind_counts.get(&stacks[si].name))
            .max()
            .copied()
            .unwrap_or(0);

        let behind_tag = if max_behind > 0 {
            format!(
                "  {}",
                format!(
                    "{max_behind} commit{} behind origin/{base}",
                    if max_behind == 1 { "" } else { "s" }
                )
                .yellow()
            )
        } else {
            String::new()
        };

        println!("{} {}{behind_tag}", "◇".cyan(), base.cyan().bold());
        total_lines += 1;

        let total_stacks = stack_indices.len();
        for (si, &stack_idx) in stack_indices.iter().enumerate() {
            let stack = &stacks[stack_idx];
            let branch_count = stack.branches.len();
            if branch_count == 0 {
                continue;
            }

            let is_last_stack = si == total_stacks - 1;
            let stack_fork = if is_last_stack { "╰─" } else { "├─" };
            let stack_pipe = if is_last_stack { "   " } else { "│  " };

            // Stack name line with behind info
            let mut stack_tags: Vec<String> = vec![];
            if let Some(&behind) = behind_counts.get(&stack.name) {
                stack_tags.push(
                    format!(
                        "{behind} behind",
                    )
                    .yellow()
                    .to_string(),
                );
            }
            let stack_tag_str = if stack_tags.is_empty() {
                String::new()
            } else {
                format!("  {}", stack_tags.join("  "))
            };

            println!(
                "{} {}{}",
                stack_fork.dimmed(),
                stack.name.magenta().bold(),
                stack_tag_str
            );
            total_lines += 1;

            // Branches under this stack
            for (idx, branch) in stack.branches.iter().enumerate() {
                let is_last_branch = idx == branch_count - 1;
                let is_current = branch.name == current_branch;
                let is_root = idx == 0;
                let info = ref_info.get(&branch.name);
                let exists = info.is_some();

                let branch_fork = if is_last_branch { "╰─" } else { "├─" };
                let branch_pipe = if is_last_branch { "   " } else { "│  " };

                let stale = needs_rebase.get(&branch.name).copied().unwrap_or(false);
                let line = format_branch_line(
                    &branch.name,
                    is_current,
                    is_root,
                    exists,
                    info,
                    is_last_branch,
                    None,
                    stale,
                );
                println!("{}{}", stack_pipe.dimmed(), line);
                branch_positions.insert(branch.name.clone(), total_lines);
                total_lines += 1;

                // Commits
                if let Some(commits) = commit_cache.get(&branch.name) {
                    for (sha, subject) in commits {
                        println!(
                            "{}{}{} {} {}",
                            stack_pipe.dimmed(),
                            branch_pipe.dimmed(),
                            "│".dimmed(),
                            sha.yellow(),
                            subject.dimmed()
                        );
                        total_lines += 1;
                    }
                }
            }
        }
    }

    // Retroactive PR update
    if show_pr && !pr_status.is_empty() {
        for (_, stack_indices) in &by_base {
            for &stack_idx in stack_indices {
                let stack = &stacks[stack_idx];
                let branch_count = stack.branches.len();
                let is_last_stack = stack_indices.last() == Some(&stack_idx);
                let stack_pipe = if is_last_stack { "   " } else { "│  " };

                for (idx, branch) in stack.branches.iter().enumerate() {
                    if let Some(pr) = pr_status.get(&branch.name) {
                        if let Some(&line_pos) = branch_positions.get(&branch.name) {
                            let is_last_branch = idx == branch_count - 1;
                            let is_current = branch.name == current_branch;
                            let is_root = idx == 0;
                            let info = ref_info.get(&branch.name);
                            let exists = info.is_some();

                            let lines_up = total_lines - line_pos;
                            let stale = needs_rebase.get(&branch.name).copied().unwrap_or(false);
                            let new_line = format_branch_line(
                                &branch.name,
                                is_current,
                                is_root,
                                exists,
                                info,
                                is_last_branch,
                                Some(pr),
                                stale,
                            );
                            print!(
                                "\x1b[{lines_up}A\r\x1b[2K{}{new_line}\x1b[{lines_up}B\r",
                                stack_pipe.dimmed()
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn format_branch_line(
    name: &str,
    is_current: bool,
    is_root: bool,
    exists: bool,
    info: Option<&RefInfo>,
    is_last: bool,
    pr: Option<&gh::PrInfo>,
    needs_rebase: bool,
) -> String {
    let marker = if is_current {
        "@".green().bold().to_string()
    } else {
        "◆".blue().to_string()
    };

    let name_str = if is_current {
        name.green().bold().to_string()
    } else if !exists {
        name.red().strikethrough().to_string()
    } else {
        name.white().bold().to_string()
    };

    let mut tags: Vec<String> = vec![];
    if is_root {
        tags.push("root".blue().dimmed().to_string());
    }
    if let Some(pr) = pr {
        tags.push(gh::format_pr_status(pr).magenta().to_string());
    }
    if let Some(ri) = info {
        match ri.remote_status {
            RemoteStatus::Diverged => tags.push("diverged".yellow().to_string()),
            RemoteStatus::NeedsPush => tags.push("needs push".yellow().to_string()),
            _ => {}
        }
    }
    if needs_rebase {
        tags.push("needs rebase".yellow().to_string());
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
    format!("{} {marker} {name_str}{tag_str}", fork.dimmed())
}

#[derive(Debug)]
enum RemoteStatus {
    UpToDate,
    NeedsPush,
    Diverged,
    NoRemote,
}

#[derive(Debug)]
struct RefInfo {
    remote_status: RemoteStatus,
}

fn batch_ref_info(ctx: &Ctx) -> HashMap<String, RefInfo> {
    let mut result = HashMap::new();
    let output = ctx.git.run(&[
        "for-each-ref",
        "--format=%(refname:short)\t%(objectname:short)\t%(upstream:short)\t%(upstream:track)",
        "refs/heads/",
    ]);
    let output = match output {
        Ok(o) => o,
        Err(_) => return result,
    };
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 {
            if let Some(name) = parts.first() {
                result.insert(name.to_string(), RefInfo { remote_status: RemoteStatus::NoRemote });
            }
            continue;
        }
        let name = parts[0];
        let upstream = parts[2];
        let track = parts[3];
        let remote_status = if upstream.is_empty() {
            RemoteStatus::NoRemote
        } else if track.is_empty() {
            RemoteStatus::UpToDate
        } else if track.contains("ahead") && track.contains("behind") {
            RemoteStatus::Diverged
        } else if track.contains("ahead") {
            RemoteStatus::NeedsPush
        } else {
            RemoteStatus::UpToDate
        };
        result.insert(name.to_string(), RefInfo { remote_status });
    }
    result
}

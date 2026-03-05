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

    // Group stacks by base
    let mut by_base: Vec<(String, Vec<usize>)> = vec![];
    for (i, stack) in stacks.iter().enumerate() {
        if let Some(entry) = by_base.iter_mut().find(|(b, _)| *b == stack.base_branch) {
            entry.1.push(i);
        } else {
            by_base.push((stack.base_branch.clone(), vec![i]));
        }
    }

    // Print the tree immediately (no PR info yet)
    // Track which output lines correspond to which branches so we can update them
    let mut total_lines = 0;
    let mut branch_line_offsets: Vec<(String, usize)> = vec![]; // (branch_name, line_from_bottom)

    for (bi, (base, stack_indices)) in by_base.iter().enumerate() {
        for &stack_idx in stack_indices {
            let stack = &stacks[stack_idx];
            let branch_count = stack.branches.len();
            if branch_count == 0 {
                continue;
            }

            if total_lines > 0 {
                println!();
                total_lines += 1;
            }

            println!("{}  {}", "◇".cyan(), base.cyan().bold());
            total_lines += 1;

            for (idx, branch) in stack.branches.iter().enumerate() {
                let is_last = idx == branch_count - 1;
                let is_current = branch.name == current_branch;
                let is_root = idx == 0;
                let info = ref_info.get(&branch.name);
                let exists = info.is_some();

                let line = format_branch_line(
                    &branch.name,
                    is_current,
                    is_root,
                    exists,
                    info,
                    is_last,
                    None,
                );
                println!("{line}");
                branch_line_offsets.push((branch.name.clone(), 0)); // placeholder
                total_lines += 1;

                if let Some(commits) = commit_cache.get(&branch.name) {
                    let pipe = if is_last { " " } else { "│" };
                    for (sha, subject) in commits {
                        println!(
                            "{}   {} {} {}",
                            pipe.dimmed(),
                            "│".dimmed(),
                            sha.yellow(),
                            subject.dimmed()
                        );
                        total_lines += 1;
                    }
                }

                if !is_last {
                    let pipe = if is_last { " " } else { "│" };
                    println!("{}", pipe.dimmed());
                    total_lines += 1;
                }
            }
        }
    }

    // Now compute line offsets from the bottom for each branch
    // We need to know how many lines from the end each branch line is
    // so we can move the cursor up to overwrite it
    let mut offset_from_end = total_lines;
    let mut cursor = 0;
    let mut branch_positions: HashMap<String, usize> = HashMap::new();

    // Re-walk the structure to find positions
    let mut line_idx = 0;
    for (base, stack_indices) in &by_base {
        for &stack_idx in stack_indices {
            let stack = &stacks[stack_idx];
            let branch_count = stack.branches.len();
            if branch_count == 0 {
                continue;
            }

            if line_idx > 0 {
                line_idx += 1; // blank line
            }
            line_idx += 1; // base header

            for (idx, branch) in stack.branches.iter().enumerate() {
                let is_last = idx == branch_count - 1;

                branch_positions.insert(branch.name.clone(), line_idx);
                line_idx += 1; // branch line

                if let Some(commits) = commit_cache.get(&branch.name) {
                    line_idx += commits.len(); // commit lines
                }

                if !is_last {
                    line_idx += 1; // spacing line
                }
            }
        }
    }

    // If PR display requested, fetch and update in-place
    if show_pr {
        let pr_status = gh::batch_pr_status();
        if !pr_status.is_empty() {
            // Move cursor up and redraw branch lines that have PR info
            for (base, stack_indices) in &by_base {
                for &stack_idx in stack_indices {
                    let stack = &stacks[stack_idx];
                    let branch_count = stack.branches.len();

                    for (idx, branch) in stack.branches.iter().enumerate() {
                        if let Some(pr) = pr_status.get(&branch.name) {
                            if let Some(&line_pos) = branch_positions.get(&branch.name) {
                                let is_last = idx == branch_count - 1;
                                let is_current = branch.name == current_branch;
                                let is_root = idx == 0;
                                let info = ref_info.get(&branch.name);
                                let exists = info.is_some();

                                let lines_up = total_lines - line_pos;
                                let new_line = format_branch_line(
                                    &branch.name,
                                    is_current,
                                    is_root,
                                    exists,
                                    info,
                                    is_last,
                                    Some(pr),
                                );

                                // ANSI: move up N lines, clear line, print, move down
                                print!("\x1b[{lines_up}A\r\x1b[2K{new_line}\x1b[{lines_up}B\r");
                            }
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
    if !exists {
        tags.push("missing".red().to_string());
    }
    let tag_str = if tags.is_empty() {
        String::new()
    } else {
        format!("  {}", tags.join("  "))
    };

    let fork = if is_last { "╰─" } else { "├─" };
    format!("{}  {marker}  {name_str}{tag_str}", fork.dimmed())
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
                result.insert(
                    name.to_string(),
                    RefInfo { remote_status: RemoteStatus::NoRemote },
                );
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

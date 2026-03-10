use std::collections::HashMap;
use std::fmt::Write;
use std::process::Child;

use anyhow::Result;
use colored::Colorize;

use crate::context::Ctx;
use crate::gh;
use crate::git::Git;
use crate::ui;

pub fn run(ctx: &Ctx, show_pr: bool, no_pager: bool) -> Result<()> {
    let stacks = ctx.load_all_stacks()?;

    if stacks.is_empty() {
        ui::info("No stacks. Create one with `gw stack create <name>`.");
        return Ok(());
    }

    // === Phase 1: current branch + all branch SHAs in parallel ===
    let branch_child = ctx.git.spawn(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    let ref_child = ctx.git.spawn(&[
        "for-each-ref",
        "--format=%(refname:short)\t%(objectname)\t%(upstream:short)\t%(upstream:track)",
        "refs/heads/",
    ])?;
    let current_branch = Git::collect(branch_child).unwrap_or_default();
    let ref_info = parse_ref_info(Git::collect(ref_child).ok());

    // === Phase 2: spawn all needed git calls as parallel subprocesses ===
    let mut mb_children: Vec<(String, Child)> = Vec::new();
    let mut log_children: Vec<(String, Child)> = Vec::new();
    let mut behind_children: Vec<(String, Child)> = Vec::new();

    for stack in &stacks {
        // Behind-count: spawn merge-base + conditional rev-list as one call
        if let Some(root) = stack.branches.first() {
            if ref_info.contains_key(&root.name) {
                if ref_info.contains_key(&stack.base_branch) {
                    if let Ok(child) = ctx.git.spawn(&[
                        "merge-base",
                        &root.name,
                        &stack.base_branch,
                    ]) {
                        behind_children.push((stack.name.clone(), child));
                    }
                    // Also need merge-base for root branch (for needs-rebase check)
                    if let Ok(child) = ctx.git.spawn(&[
                        "merge-base",
                        &root.name,
                        &stack.base_branch,
                    ]) {
                        let key = format!("{}:{}", root.name, stack.base_branch);
                        mb_children.push((key, child));
                    }
                }
            }
        }

        for (idx, branch) in stack.branches.iter().enumerate() {
            if !ref_info.contains_key(&branch.name) {
                continue;
            }
            let parent = stack.parent_of(&branch.name).unwrap_or_default();

            // Merge-base for non-root branches
            if idx > 0 {
                if let Ok(child) =
                    ctx.git.spawn(&["merge-base", &branch.name, &parent])
                {
                    let key = format!("{}:{}", branch.name, parent);
                    mb_children.push((key, child));
                }
            }

            // Log
            let range = format!("{parent}..{}", branch.name);
            if let Ok(child) = ctx.git.spawn(&[
                "log",
                "--reverse",
                "--oneline",
                "--format=%h %s",
                "--max-count=10",
                &range,
            ]) {
                log_children.push((branch.name.clone(), child));
            }
        }
    }

    // === Phase 3: collect results ===
    let mut merge_bases: HashMap<String, String> = HashMap::new();
    for (key, child) in mb_children {
        if let Ok(sha) = Git::collect(child) {
            if !sha.is_empty() {
                merge_bases.insert(key, sha);
            }
        }
    }

    let mut commit_cache: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (branch, child) in log_children {
        if let Ok(output) = Git::collect(child) {
            let commits: Vec<(String, String)> = output
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let (sha, subject) = line.split_once(' ').unwrap_or((line, ""));
                    (sha.to_string(), subject.to_string())
                })
                .collect();
            commit_cache.insert(branch, commits);
        }
    }

    let mut behind_counts: HashMap<String, usize> = HashMap::new();
    for (stack_name, child) in behind_children {
        if let Ok(mb_sha) = Git::collect(child) {
            let stack = stacks.iter().find(|s| s.name == stack_name);
            if let Some(stack) = stack {
                if let Some(base_ri) = ref_info.get(&stack.base_branch) {
                    if mb_sha != base_ri.sha && !mb_sha.is_empty() {
                        let range = format!("{mb_sha}..{}", stack.base_branch);
                        if let Ok(count_str) =
                            ctx.git.run(&["rev-list", "--count", &range])
                        {
                            if let Ok(n) = count_str.trim().parse::<usize>() {
                                if n > 0 {
                                    behind_counts.insert(stack_name, n);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // === Phase 4: compute derived data ===
    let mut needs_rebase: HashMap<String, bool> = HashMap::new();
    for stack in &stacks {
        for (idx, branch) in stack.branches.iter().enumerate() {
            if idx == 0 || !ref_info.contains_key(&branch.name) {
                continue;
            }
            let parent_name = stack.parent_of(&branch.name).unwrap_or_default();
            let pair_key = format!("{}:{}", branch.name, parent_name);
            if let Some(mb_sha) = merge_bases.get(&pair_key) {
                if let Some(ri) = ref_info.get(&parent_name) {
                    if *mb_sha != ri.sha {
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

    // PR status (only when requested)
    let pr_status = if show_pr {
        let branch_names: Vec<&str> = stacks
            .iter()
            .flat_map(|s| s.branches.iter().map(|b| b.name.as_str()))
            .collect();
        gh::batch_pr_status(&branch_names)
    } else {
        HashMap::new()
    };

    // === Render to buffer ===
    let mut buf = String::new();
    let mut current_branch_line: Option<usize> = None;
    let mut line_num: usize = 0;

    for (base, stack_indices) in &by_base {
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

        let _ = writeln!(buf, "{} {}{behind_tag}", "◇".cyan(), base.cyan().bold());
        line_num += 1;

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

            let mut stack_tags: Vec<String> = vec![];
            if let Some(&behind) = behind_counts.get(&stack.name) {
                stack_tags.push(format!("{behind} behind").yellow().to_string());
            }
            let stack_tag_str = if stack_tags.is_empty() {
                String::new()
            } else {
                format!("  {}", stack_tags.join("  "))
            };

            let _ = writeln!(
                buf,
                "{} {}{}",
                stack_fork.dimmed(),
                stack.name.magenta().bold(),
                stack_tag_str
            );
            line_num += 1;

            for (idx, branch) in stack.branches.iter().enumerate() {
                let is_last_branch = idx == branch_count - 1;
                let is_current = branch.name == current_branch;
                let is_root = idx == 0;
                let info = ref_info.get(&branch.name);
                let exists = info.is_some();

                let branch_pipe = if is_last_branch { "   " } else { "│  " };

                let stale = needs_rebase.get(&branch.name).copied().unwrap_or(false);
                let line = format_branch_line(
                    &branch.name,
                    is_current,
                    is_root,
                    exists,
                    info,
                    is_last_branch,
                    pr_status.get(&branch.name),
                    stale,
                );
                let _ = writeln!(buf, "{}{}", stack_pipe.dimmed(), line);
                line_num += 1;
                if is_current {
                    current_branch_line = Some(line_num);
                }

                if let Some(commits) = commit_cache.get(&branch.name) {
                    for (sha, subject) in commits {
                        let _ = writeln!(
                            buf,
                            "{}{}{} {} {}",
                            stack_pipe.dimmed(),
                            branch_pipe.dimmed(),
                            "│".dimmed(),
                            sha.yellow(),
                            subject.dimmed()
                        );
                        line_num += 1;
                    }
                }
            }
        }
    }

    ui::output_with_pager(&buf, no_pager, current_branch_line);

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
            RemoteStatus::Behind => tags.push("behind remote".yellow().to_string()),
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
    Behind,
    Diverged,
    NoRemote,
}

#[derive(Debug)]
struct RefInfo {
    sha: String,
    remote_status: RemoteStatus,
}

fn parse_ref_info(output: Option<String>) -> HashMap<String, RefInfo> {
    let mut result = HashMap::new();
    let output = match output {
        Some(o) => o,
        None => return result,
    };
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.is_empty() {
            continue;
        }
        let name = parts[0];
        let sha = parts.get(1).unwrap_or(&"").to_string();
        let upstream = parts.get(2).unwrap_or(&"");
        let track = parts.get(3).unwrap_or(&"");
        let remote_status = if upstream.is_empty() {
            RemoteStatus::NoRemote
        } else if track.is_empty() {
            RemoteStatus::UpToDate
        } else if track.contains("gone") {
            RemoteStatus::NoRemote
        } else if track.contains("ahead") && track.contains("behind") {
            RemoteStatus::Diverged
        } else if track.contains("ahead") {
            RemoteStatus::NeedsPush
        } else if track.contains("behind") {
            RemoteStatus::Behind
        } else {
            RemoteStatus::UpToDate
        };
        result.insert(name.to_string(), RefInfo { sha, remote_status });
    }
    result
}

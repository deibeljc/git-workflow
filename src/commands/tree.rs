use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use crate::context::Ctx;
use crate::gh;
use crate::ui;

/// All the git data we need, fetched in as few subprocesses as possible.
struct TreeData {
    merge_bases: HashMap<String, String>,
    commit_cache: HashMap<String, Vec<(String, String)>>,
    behind_counts: HashMap<String, usize>,
}

pub fn run(ctx: &Ctx, show_pr: bool) -> Result<()> {
    let stacks = ctx.load_all_stacks()?;

    if stacks.is_empty() {
        ui::info("No stacks. Create one with `gw stack create <name>`.");
        return Ok(());
    }

    let current_branch = ctx.git.current_branch().unwrap_or_default();

    // === Phase 1: one git call for all branch SHAs + tracking info ===
    let ref_info = batch_ref_info(ctx);

    // === Phase 2: figure out what we need ===
    let mut merge_base_pairs: Vec<(String, String)> = Vec::new();
    let mut log_ranges: Vec<(String, String)> = Vec::new(); // (branch, parent)

    for stack in &stacks {
        if let Some(root) = stack.branches.first() {
            if ref_info.contains_key(&root.name) {
                merge_base_pairs.push((root.name.clone(), stack.base_branch.clone()));
            }
        }
        for (idx, branch) in stack.branches.iter().enumerate() {
            if !ref_info.contains_key(&branch.name) {
                continue;
            }
            let parent = stack.parent_of(&branch.name).unwrap_or_default();
            if idx > 0 {
                merge_base_pairs.push((branch.name.clone(), parent.clone()));
            }
            log_ranges.push((branch.name.clone(), parent.clone()));
        }
    }

    // === Phase 3: ONE bash subprocess, all git calls in parallel ===
    let batch = run_batch_script(ctx, &ref_info, &stacks, &merge_base_pairs, &log_ranges);

    // === Phase 4: compute derived data ===
    let mut needs_rebase: HashMap<String, bool> = HashMap::new();
    for stack in &stacks {
        for (idx, branch) in stack.branches.iter().enumerate() {
            if idx == 0 || !ref_info.contains_key(&branch.name) {
                continue;
            }
            let parent_name = stack.parent_of(&branch.name).unwrap_or_default();
            let pair_key = format!("{}:{}", branch.name, parent_name);
            if let Some(mb_sha) = batch.merge_bases.get(&pair_key) {
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
        gh::batch_pr_status()
    } else {
        HashMap::new()
    };

    // === Render ===
    let mut total_lines = 0;
    let mut branch_positions: HashMap<String, usize> = HashMap::new();

    for (base, stack_indices) in &by_base {
        let max_behind = stack_indices
            .iter()
            .filter_map(|&si| batch.behind_counts.get(&stacks[si].name))
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

            let mut stack_tags: Vec<String> = vec![];
            if let Some(&behind) = batch.behind_counts.get(&stack.name) {
                stack_tags.push(format!("{behind} behind").yellow().to_string());
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
                    None,
                    stale,
                );
                println!("{}{}", stack_pipe.dimmed(), line);
                branch_positions.insert(branch.name.clone(), total_lines);
                total_lines += 1;

                if let Some(commits) = batch.commit_cache.get(&branch.name) {
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
                            let stale =
                                needs_rebase.get(&branch.name).copied().unwrap_or(false);
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

/// Run a single bash subprocess that executes all merge-bases, logs, and
/// behind-counts in parallel using temp files to avoid output interleaving.
fn run_batch_script(
    ctx: &Ctx,
    ref_info: &HashMap<String, RefInfo>,
    stacks: &[crate::state::StackConfig],
    merge_base_pairs: &[(String, String)],
    log_ranges: &[(String, String)],
) -> TreeData {
    let mut script = String::from("TD=$(mktemp -d)\n");

    // --- All parallel jobs write to individual temp files ---

    // Merge-bases: write to $TD/mb_<idx>
    for (i, (a, b)) in merge_base_pairs.iter().enumerate() {
        script.push_str(&format!(
            "{{ echo \"{a}:{b}=$(git merge-base {a} {b} 2>/dev/null)\"; }} > \"$TD/mb_{i}\" &\n"
        ));
    }

    // Logs: write to $TD/log_<branch>
    for (branch, parent) in log_ranges {
        script.push_str(&format!(
            "git log --reverse --oneline --format='%h %s' --max-count=10 \
             {parent}..{branch} 2>/dev/null > \"$TD/log_{branch}\" &\n"
        ));
    }

    // Behind-counts: write to $TD/behind_<stack_name>
    for stack in stacks {
        if let Some(root) = stack.branches.first() {
            if ref_info.contains_key(&root.name) {
                if let Some(base_ri) = ref_info.get(&stack.base_branch) {
                    let root_name = &root.name;
                    let base = &stack.base_branch;
                    let base_sha = &base_ri.sha;
                    let stack_name = &stack.name;
                    script.push_str(&format!(
                        "{{ MB=$(git merge-base {root_name} {base} 2>/dev/null); \
                         if [ \"$MB\" != \"{base_sha}\" ] && [ -n \"$MB\" ]; then \
                         git rev-list --count \"$MB\"..{base} 2>/dev/null; \
                         fi; }} > \"$TD/behind_{stack_name}\" &\n"
                    ));
                }
            }
        }
    }

    script.push_str("wait\n");

    // --- Collect results to stdout in structured format ---
    // Merge-bases
    for i in 0..merge_base_pairs.len() {
        script.push_str(&format!("echo \"MB:$(cat \"$TD/mb_{i}\")\"\n"));
    }
    // Logs
    for (branch, _) in log_ranges {
        script.push_str(&format!(
            "echo \"LOG:{branch}\"\ncat \"$TD/log_{branch}\"\necho \"LOG_END\"\n"
        ));
    }
    // Behind-counts
    for stack in stacks {
        if let Some(root) = stack.branches.first() {
            if ref_info.contains_key(&root.name) {
                if ref_info.contains_key(&stack.base_branch) {
                    let stack_name = &stack.name;
                    script.push_str(&format!(
                        "V=$(cat \"$TD/behind_{stack_name}\" 2>/dev/null); \
                         [ -n \"$V\" ] && echo \"BEHIND:{stack_name}=$V\"\n"
                    ));
                }
            }
        }
    }

    script.push_str("rm -rf \"$TD\"\n");

    // Run it
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(&script)
        .current_dir(ctx.git.repo_path())
        .output();

    let mut merge_bases = HashMap::new();
    let mut commit_cache: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut behind_counts = HashMap::new();

    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut current_log_branch: Option<String> = None;
        let mut current_commits: Vec<(String, String)> = Vec::new();

        for line in stdout.lines() {
            if let Some(rest) = line.strip_prefix("MB:") {
                if let Some((key, sha)) = rest.split_once('=') {
                    if !sha.is_empty() {
                        merge_bases.insert(key.to_string(), sha.to_string());
                    }
                }
            } else if let Some(rest) = line.strip_prefix("LOG:") {
                if let Some(branch) = current_log_branch.take() {
                    commit_cache.insert(branch, std::mem::take(&mut current_commits));
                }
                current_log_branch = Some(rest.to_string());
                current_commits.clear();
            } else if line == "LOG_END" {
                if let Some(branch) = current_log_branch.take() {
                    commit_cache.insert(branch, std::mem::take(&mut current_commits));
                }
            } else if let Some(rest) = line.strip_prefix("BEHIND:") {
                if let Some((name, count_str)) = rest.split_once('=') {
                    if let Ok(n) = count_str.trim().parse::<usize>() {
                        if n > 0 {
                            behind_counts.insert(name.to_string(), n);
                        }
                    }
                }
            } else if current_log_branch.is_some() && !line.is_empty() {
                let (sha, subject) = line.split_once(' ').unwrap_or((line, ""));
                current_commits.push((sha.to_string(), subject.to_string()));
            }
        }
        if let Some(branch) = current_log_branch.take() {
            commit_cache.insert(branch, current_commits);
        }
    }

    TreeData {
        merge_bases,
        commit_cache,
        behind_counts,
    }
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
    sha: String,
    remote_status: RemoteStatus,
}

/// Single git call to get all branch SHAs and remote tracking info.
fn batch_ref_info(ctx: &Ctx) -> HashMap<String, RefInfo> {
    let mut result = HashMap::new();
    let output = ctx.git.run(&[
        "for-each-ref",
        "--format=%(refname:short)\t%(objectname)\t%(upstream:short)\t%(upstream:track)",
        "refs/heads/",
    ]);
    let output = match output {
        Ok(o) => o,
        Err(_) => return result,
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
        } else if track.contains("ahead") && track.contains("behind") {
            RemoteStatus::Diverged
        } else if track.contains("ahead") {
            RemoteStatus::NeedsPush
        } else {
            RemoteStatus::UpToDate
        };
        result.insert(name.to_string(), RefInfo { sha, remote_status });
    }
    result
}

use anyhow::{bail, Result};
use colored::Colorize;

use crate::context::Ctx;

pub fn run(ctx: &Ctx) -> Result<()> {
    let current = ctx.git.current_branch()?;

    let stack = match ctx.find_stack_for_branch(&current)? {
        Some(s) => s,
        None => bail!(
            "Branch '{}' is not in any stack.\n\
             Use `gw adopt` to add it.",
            current
        ),
    };

    let idx = stack.branch_index(&current).unwrap();
    let total = stack.branches.len();
    let parent = stack.parent_of(&current).unwrap();
    let children: Vec<&str> = if idx + 1 < total {
        vec![&stack.branches[idx + 1].name]
    } else {
        vec![]
    };

    let is_root = idx == 0;
    let is_leaf = idx == total - 1;

    // Gather git info
    let merge_base = ctx.git.merge_base(&parent, &current)?;
    let commits = ctx.git.log_oneline(&merge_base, "HEAD", 100)?;
    let behind_parent: usize = ctx
        .git
        .run(&["rev-list", "--count", &format!("{merge_base}..{parent}")])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let needs_rebase = behind_parent > 0;

    // Remote status
    let remote_status = remote_info(ctx, &current);

    // Working tree
    let wt = working_tree_info(ctx);

    // === Render ===

    // Branch + stack header
    let position_label = if is_root && is_leaf {
        "only branch".dimmed().to_string()
    } else if is_root {
        "root".blue().dimmed().to_string()
    } else if is_leaf {
        "leaf".blue().dimmed().to_string()
    } else {
        format!("{}", format!("{} of {total}", idx + 1).dimmed())
    };

    println!(
        "{}  {}  {}",
        format!("@").green().bold(),
        current.green().bold(),
        position_label,
    );

    println!(
        "{}  {}",
        "stack".dimmed(),
        stack.name.magenta().bold(),
    );

    // Navigation: parent and child
    println!(
        "   {}  {}",
        "↑".dimmed(),
        if is_root {
            format!("{} {}", parent.cyan(), "(base)".dimmed())
        } else {
            parent.white().bold().to_string()
        },
    );

    if let Some(child) = children.first() {
        println!(
            "   {}  {}",
            "↓".dimmed(),
            child.white().bold(),
        );
    }

    println!();

    // Commits
    let commit_label = format!(
        "{} commit{}",
        commits.len(),
        if commits.len() == 1 { "" } else { "s" }
    );
    println!("{}", commit_label.dimmed());
    for (sha, subject) in &commits {
        println!("  {} {}", sha.yellow(), subject);
    }
    if commits.is_empty() {
        println!("  {}", "(no commits yet)".dimmed());
    }

    println!();

    // Status flags
    let mut flags: Vec<String> = vec![];

    match &wt {
        WorkingTree::Clean => flags.push(format!("{}", "clean".green())),
        WorkingTree::Dirty { staged, unstaged, untracked } => {
            let mut parts: Vec<String> = vec![];
            if *staged > 0 {
                parts.push(format!("{} staged", staged).green().to_string());
            }
            if *unstaged > 0 {
                parts.push(format!("{} modified", unstaged).yellow().to_string());
            }
            if *untracked > 0 {
                parts.push(format!("{} untracked", untracked).dimmed().to_string());
            }
            flags.push(parts.join("  "));
        }
    }

    match &remote_status {
        RemoteStatus::UpToDate => flags.push(format!("{}", "pushed".green())),
        RemoteStatus::NeedsPush(ahead) => {
            flags.push(format!("{}", format!("{ahead} ahead, needs push").yellow()));
        }
        RemoteStatus::Behind(behind) => {
            flags.push(format!("{}", format!("{behind} behind remote").yellow()));
        }
        RemoteStatus::Diverged { ahead, behind } => {
            flags.push(
                format!("diverged ({ahead} ahead, {behind} behind remote)")
                    .yellow()
                    .to_string(),
            );
        }
        RemoteStatus::NoRemote => {
            flags.push(format!("{}", "not pushed".dimmed()));
        }
    }

    if needs_rebase {
        flags.push(
            format!("{behind_parent} behind parent, needs rebase")
                .yellow()
                .to_string(),
        );
    }

    for flag in &flags {
        println!("  {} {flag}", "•".dimmed());
    }

    Ok(())
}

enum WorkingTree {
    Clean,
    Dirty {
        staged: usize,
        unstaged: usize,
        untracked: usize,
    },
}

fn working_tree_info(ctx: &Ctx) -> WorkingTree {
    let output = ctx.git.run(&["status", "--porcelain"]);
    let output = match output {
        Ok(o) if o.is_empty() => return WorkingTree::Clean,
        Ok(o) => o,
        Err(_) => return WorkingTree::Clean,
    };

    let mut staged = 0usize;
    let mut unstaged = 0usize;
    let mut untracked = 0usize;

    for line in output.lines() {
        if line.len() < 2 {
            continue;
        }
        let x = line.as_bytes()[0];
        let y = line.as_bytes()[1];

        if x == b'?' {
            untracked += 1;
        } else {
            if x != b' ' && x != b'?' {
                staged += 1;
            }
            if y != b' ' && y != b'?' {
                unstaged += 1;
            }
        }
    }

    WorkingTree::Dirty {
        staged,
        unstaged,
        untracked,
    }
}

enum RemoteStatus {
    UpToDate,
    NeedsPush(usize),
    Behind(usize),
    Diverged { ahead: usize, behind: usize },
    NoRemote,
}

fn remote_info(ctx: &Ctx, branch: &str) -> RemoteStatus {
    let output = ctx.git.run(&[
        "for-each-ref",
        &format!("--format=%(upstream:short)\t%(upstream:track)"),
        &format!("refs/heads/{branch}"),
    ]);

    let output = match output {
        Ok(o) => o,
        Err(_) => return RemoteStatus::NoRemote,
    };

    let parts: Vec<&str> = output.split('\t').collect();
    let upstream = parts.first().unwrap_or(&"");
    let track = parts.get(1).unwrap_or(&"");

    if upstream.is_empty() {
        return RemoteStatus::NoRemote;
    }

    if track.is_empty() {
        return RemoteStatus::UpToDate;
    }

    if track.contains("gone") {
        return RemoteStatus::NoRemote;
    }

    if track.contains("ahead") && track.contains("behind") {
        let ahead = parse_count_after(track, "ahead ");
        let behind = parse_count_after(track, "behind ");
        return RemoteStatus::Diverged { ahead, behind };
    }

    if track.contains("ahead") {
        let ahead = parse_count_after(track, "ahead ");
        return RemoteStatus::NeedsPush(ahead);
    }

    if track.contains("behind") {
        let behind = parse_count_after(track, "behind ");
        return RemoteStatus::Behind(behind);
    }

    RemoteStatus::UpToDate
}

fn parse_count_after(s: &str, prefix: &str) -> usize {
    s.split(prefix)
        .nth(1)
        .and_then(|rest| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_count_after() {
        assert_eq!(parse_count_after("[ahead 3]", "ahead "), 3);
        assert_eq!(parse_count_after("[behind 7]", "behind "), 7);
        assert_eq!(parse_count_after("[ahead 2, behind 5]", "ahead "), 2);
        assert_eq!(parse_count_after("[ahead 2, behind 5]", "behind "), 5);
        assert_eq!(parse_count_after("[ahead 100]", "ahead "), 100);
        assert_eq!(parse_count_after("garbage", "ahead "), 1); // fallback
    }
}

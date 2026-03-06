use anyhow::{bail, Result};
use colored::Colorize;
use console::{Key, Term};
use std::collections::HashSet;

use crate::context::Ctx;
use crate::stash;
use crate::ui;

struct PickerItem {
    /// Colored display text for inactive state.
    colored: String,
    /// Plain text for active (yellow) state.
    plain: String,
    /// Branch name this item maps to, or None if non-selectable (header).
    branch: Option<String>,
}

pub fn run(branch: Option<String>, ctx: &Ctx) -> Result<()> {
    let stacks = ctx.load_all_stacks()?;

    if stacks.is_empty() {
        bail!("No stacks. Create one with `gw stack create <name>`.");
    }

    let current = ctx.git.current_branch().unwrap_or_default();

    // Collect base branches (deduplicated) and all tracked branches
    let mut seen_bases: HashSet<String> = HashSet::new();
    let mut all_branch_names: Vec<String> = vec![];

    for stack in &stacks {
        seen_bases.insert(stack.base_branch.clone());
        for b in &stack.branches {
            all_branch_names.push(b.name.clone());
        }
    }

    if let Some(target) = branch {
        // Direct mode: check base branches and tracked branches
        let is_base = seen_bases.contains(&target);
        let is_tracked = all_branch_names.contains(&target);
        if !is_base && !is_tracked {
            bail!(
                "Branch '{target}' is not tracked by gw. Use `git checkout` for untracked branches."
            );
        }
        if target == current {
            ui::info("Already on that branch.");
            return Ok(());
        }
        stash::checkout_with_stash(ctx, &target)?;
        ui::success(&format!("Switched to {target}"));
        return Ok(());
    }

    // Interactive mode
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        bail!("Interactive mode requires a terminal. Pass a branch name: `gw switch <branch>`");
    }

    // Build tree-style display matching `gw tree` structure (without commits)
    let mut items: Vec<PickerItem> = vec![];

    // Group stacks by base branch (same logic as tree.rs)
    let mut by_base: Vec<(String, Vec<usize>)> = vec![];
    for (i, stack) in stacks.iter().enumerate() {
        if let Some(entry) = by_base.iter_mut().find(|(b, _)| *b == stack.base_branch) {
            entry.1.push(i);
        } else {
            by_base.push((stack.base_branch.clone(), vec![i]));
        }
    }

    for (base, stack_indices) in &by_base {
        // Base branch header (selectable)
        items.push(PickerItem {
            colored: format!("{} {}", "◇".cyan(), base.cyan().bold()),
            plain: format!("◇ {base}"),
            branch: Some(base.clone()),
        });

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

            // Stack name line (non-selectable)
            items.push(PickerItem {
                colored: format!("{} {}", stack_fork.dimmed(), stack.name.magenta().bold()),
                plain: format!("{stack_fork} {}", stack.name),
                branch: None,
            });

            // Branches under this stack (selectable)
            for (idx, b) in stack.branches.iter().enumerate() {
                let is_last_branch = idx == branch_count - 1;
                let is_current = b.name == current;
                let is_root = idx == 0;

                let branch_fork = if is_last_branch { "╰─" } else { "├─" };

                let marker = if is_current {
                    "@".green().bold().to_string()
                } else {
                    "◆".blue().to_string()
                };
                let plain_marker = if is_current { "@" } else { "◆" };

                let name_str = if is_current {
                    b.name.green().bold().to_string()
                } else {
                    b.name.white().bold().to_string()
                };

                let mut tags: Vec<String> = vec![];
                let mut plain_tags: Vec<&str> = vec![];
                if is_root {
                    tags.push("root".blue().dimmed().to_string());
                    plain_tags.push("root");
                }
                let tag_str = if tags.is_empty() {
                    String::new()
                } else {
                    format!("  {}", tags.join("  "))
                };
                let plain_tag_str = if plain_tags.is_empty() {
                    String::new()
                } else {
                    format!("  {}", plain_tags.join("  "))
                };

                items.push(PickerItem {
                    colored: format!(
                        "{}{} {marker} {name_str}{tag_str}",
                        stack_pipe.dimmed(),
                        branch_fork.dimmed(),
                    ),
                    plain: format!(
                        "{stack_pipe}{branch_fork} {plain_marker} {}{plain_tag_str}",
                        b.name,
                    ),
                    branch: Some(b.name.clone()),
                });
            }
        }
    }

    // Find selectable indices
    let selectable: Vec<usize> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| item.branch.as_ref().map(|_| i))
        .collect();

    if selectable.is_empty() {
        bail!("No branches tracked by gw.");
    }

    // Start cursor on current branch, or first selectable
    let initial = selectable
        .iter()
        .position(|&i| items[i].branch.as_deref() == Some(&current))
        .unwrap_or(0);

    let selected = run_picker(&items, &selectable, initial)?;

    match selected {
        Some(idx) => {
            let target = items[idx].branch.as_ref().unwrap();
            if *target == current {
                ui::info("Already on that branch.");
            } else {
                stash::checkout_with_stash(ctx, target)?;
                ui::success(&format!("Switched to {target}"));
            }
        }
        None => {
            ui::info("Cancelled.");
        }
    }

    Ok(())
}

/// Simple arrow-key picker. Returns the selected index, or None if cancelled.
fn run_picker(
    items: &[PickerItem],
    selectable: &[usize],
    initial: usize,
) -> Result<Option<usize>> {
    let term = Term::stderr();
    let mut cursor = initial;

    // Draw all lines
    render(&term, items, selectable, cursor)?;

    loop {
        match term.read_key()? {
            Key::ArrowUp | Key::Char('k') => {
                if cursor > 0 {
                    cursor -= 1;
                    redraw(&term, items, selectable, cursor)?;
                }
            }
            Key::ArrowDown | Key::Char('j') => {
                if cursor < selectable.len() - 1 {
                    cursor += 1;
                    redraw(&term, items, selectable, cursor)?;
                }
            }
            Key::Enter => {
                // Clear the picker
                term.clear_last_lines(items.len() + 1)?;
                return Ok(Some(selectable[cursor]));
            }
            Key::Escape | Key::Char('q') => {
                term.clear_last_lines(items.len() + 1)?;
                return Ok(None);
            }
            _ => {}
        }
    }
}

fn render(
    term: &Term,
    items: &[PickerItem],
    selectable: &[usize],
    cursor: usize,
) -> Result<()> {
    term.write_line(&format!(
        "{} {}",
        "?".green().bold(),
        "Switch to branch (↑↓ navigate, enter select, q quit):".bold()
    ))?;

    let active_idx = selectable[cursor];
    for (i, item) in items.iter().enumerate() {
        if i == active_idx {
            term.write_line(&format!(
                "{} {}",
                "▸".yellow(),
                console::style(&item.plain).yellow()
            ))?;
        } else {
            term.write_line(&format!("  {}", item.colored))?;
        }
    }

    Ok(())
}

fn redraw(
    term: &Term,
    items: &[PickerItem],
    selectable: &[usize],
    cursor: usize,
) -> Result<()> {
    // Move up and redraw
    term.clear_last_lines(items.len() + 1)?;
    render(term, items, selectable, cursor)?;
    Ok(())
}

use anyhow::{bail, Result};
use colored::Colorize;
use console::{Key, Term};
use std::collections::HashSet;

use crate::commands::split::{Bucket, SplitPlan};
use crate::git::CommitInfo;
use crate::validate;

/// Assignment state for a single commit.
#[derive(Clone)]
struct CommitEntry {
    sha: String,
    subject: String,
    /// Index into `buckets` vec, or None if unassigned.
    bucket_idx: Option<usize>,
}

/// A named bucket that commits can be assigned to.
struct BucketEntry {
    name: String,
}

/// Input mode for the TUI.
enum Mode {
    /// Navigate commits, assign to buckets.
    Normal,
    /// Typing a name for a new bucket.
    TextInput,
}

/// RAII guard to restore cursor visibility on drop.
struct CursorGuard<'a>(&'a Term);

impl Drop for CursorGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.show_cursor();
    }
}

/// Run the interactive split TUI.
///
/// Takes the commits on the branch and returns a `SplitPlan` with the user's
/// bucket assignments. Returns `None` if the user cancels.
pub fn run_split_tui(
    commits: &[CommitInfo],
    existing_branches: &HashSet<String>,
) -> Result<Option<SplitPlan>> {
    let term = Term::stderr();

    if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
        bail!("Interactive mode requires a terminal. Use --plan <file> instead.");
    }

    let _guard = CursorGuard(&term);
    let _ = term.hide_cursor();

    let mut entries: Vec<CommitEntry> = commits
        .iter()
        .map(|c| CommitEntry {
            sha: c.full_sha.clone(),
            subject: c.subject.clone(),
            bucket_idx: None,
        })
        .collect();

    let mut buckets: Vec<BucketEntry> = Vec::new();
    let mut cursor: usize = 0;
    let mut mode = Mode::Normal;
    let mut input_buf = String::new();
    let mut input_error: Option<String> = None;
    let mut last_height: usize = 0;

    // Clone existing branches for collision checking
    let mut known_branches = existing_branches.clone();

    render(&term, &entries, &buckets, cursor, &mode, &input_buf, &input_error, &mut last_height)?;

    loop {
        let key = term.read_key()?;

        match mode {
            Mode::Normal => match key {
                Key::ArrowUp | Key::Char('k') => {
                    if cursor > 0 {
                        cursor -= 1;
                    }
                }
                Key::ArrowDown | Key::Char('j') => {
                    if cursor < entries.len() - 1 {
                        cursor += 1;
                    }
                }
                Key::Char(c @ '1'..='9') => {
                    let idx = (c as usize) - ('1' as usize);
                    if idx < buckets.len() {
                        entries[cursor].bucket_idx = Some(idx);
                    }
                }
                Key::Char('n') => {
                    mode = Mode::TextInput;
                    input_buf.clear();
                    input_error = None;
                }
                Key::Char('u') => {
                    entries[cursor].bucket_idx = None;
                }
                Key::Enter => {
                    // Validate: all assigned, 2+ buckets
                    let unassigned = entries.iter().filter(|e| e.bucket_idx.is_none()).count();
                    if unassigned > 0 {
                        input_error = Some(format!("{unassigned} commit(s) still unassigned."));
                    } else if buckets.len() < 2 {
                        input_error = Some("Need at least 2 buckets.".to_string());
                    } else {
                        // Success — build plan
                        clear(&term, last_height)?;
                        return Ok(Some(build_plan(&entries, &buckets)));
                    }
                }
                Key::Escape | Key::Char('q') => {
                    clear(&term, last_height)?;
                    return Ok(None);
                }
                _ => {}
            },
            Mode::TextInput => match key {
                Key::Enter => {
                    let name = input_buf.trim().to_string();
                    if name.is_empty() {
                        input_error = Some("Name cannot be empty.".to_string());
                    } else if let Err(e) = validate::validate_branch_name(&name) {
                        input_error = Some(format!("Invalid: {e}"));
                    } else if known_branches.contains(&name) {
                        input_error = Some(format!("Branch '{name}' already exists."));
                    } else if buckets.iter().any(|b| b.name == name) {
                        input_error = Some(format!("Bucket '{name}' already exists."));
                    } else {
                        // Create the bucket and assign current commit
                        let idx = buckets.len();
                        known_branches.insert(name.clone());
                        buckets.push(BucketEntry { name });
                        entries[cursor].bucket_idx = Some(idx);
                        mode = Mode::Normal;
                        input_buf.clear();
                        input_error = None;
                    }
                }
                Key::Escape => {
                    mode = Mode::Normal;
                    input_buf.clear();
                    input_error = None;
                }
                Key::Backspace => {
                    input_buf.pop();
                    input_error = None;
                }
                Key::Char(c) => {
                    input_buf.push(c);
                    input_error = None;
                }
                _ => {}
            },
        }

        render(&term, &entries, &buckets, cursor, &mode, &input_buf, &input_error, &mut last_height)?;
    }
}

/// Build a SplitPlan from the TUI state.
fn build_plan(entries: &[CommitEntry], buckets: &[BucketEntry]) -> SplitPlan {
    let mut plan_buckets: Vec<Bucket> = buckets
        .iter()
        .map(|b| Bucket {
            branch_name: b.name.clone(),
            commits: Vec::new(),
        })
        .collect();

    // Assign commits to buckets in original order
    for entry in entries {
        if let Some(idx) = entry.bucket_idx {
            plan_buckets[idx].commits.push(entry.sha.clone());
        }
    }

    // Remove empty buckets (buckets that had all commits unassigned then reassigned)
    plan_buckets.retain(|b| !b.commits.is_empty());

    SplitPlan {
        buckets: plan_buckets,
    }
}

/// Clear previously rendered lines.
fn clear(term: &Term, height: usize) -> Result<()> {
    if height > 0 {
        term.clear_last_lines(height)?;
    }
    Ok(())
}

/// Bucket display colors — cycle through these for visual distinction.
const BUCKET_COLORS: &[&str] = &["green", "blue", "magenta", "cyan", "yellow", "red"];

fn bucket_color(idx: usize) -> &'static str {
    BUCKET_COLORS[idx % BUCKET_COLORS.len()]
}

fn colorize_bucket_name(name: &str, idx: usize) -> String {
    match bucket_color(idx) {
        "green" => name.green().to_string(),
        "blue" => name.blue().to_string(),
        "magenta" => name.magenta().to_string(),
        "cyan" => name.cyan().to_string(),
        "yellow" => name.yellow().to_string(),
        "red" => name.red().to_string(),
        _ => name.to_string(),
    }
}

fn colorize_bucket_tag(name: &str, idx: usize) -> String {
    let colored_name = colorize_bucket_name(name, idx);
    format!("[{}]", colored_name)
}

/// Render the full TUI to the terminal.
fn render(
    term: &Term,
    entries: &[CommitEntry],
    buckets: &[BucketEntry],
    cursor: usize,
    mode: &Mode,
    input_buf: &str,
    input_error: &Option<String>,
    last_height: &mut usize,
) -> Result<()> {
    // Clear previous render
    clear(term, *last_height)?;

    let mut lines: Vec<String> = Vec::new();

    // Header
    lines.push(format!(
        "{} {}",
        "gw split".cyan().bold(),
        "— Assign commits to branches".dimmed()
    ));
    lines.push(String::new());

    // Commits list
    let (term_height, _term_width) = term.size();
    let max_visible = (term_height as usize).saturating_sub(12).min(30).max(5);

    // Calculate viewport
    let total = entries.len();
    let (view_start, view_end) = if total <= max_visible {
        (0, total)
    } else {
        let half = max_visible / 2;
        let start = if cursor <= half {
            0
        } else if cursor + half >= total {
            total.saturating_sub(max_visible)
        } else {
            cursor - half
        };
        (start, (start + max_visible).min(total))
    };

    if view_start > 0 {
        lines.push(format!("  {} more above", format!("↑ {}", view_start).dimmed()));
    }

    for i in view_start..view_end {
        let entry = &entries[i];
        let short_sha = &entry.sha[..7.min(entry.sha.len())];
        let is_selected = i == cursor;

        let marker = if is_selected { "▸" } else { " " };
        let marker_str = if is_selected {
            marker.yellow().bold().to_string()
        } else {
            marker.to_string()
        };

        let dot = if entry.bucket_idx.is_some() {
            "●".to_string()
        } else {
            "○".dimmed().to_string()
        };

        let sha_str = if is_selected {
            short_sha.yellow().to_string()
        } else {
            short_sha.dimmed().to_string()
        };

        let subject_str = if is_selected {
            entry.subject.yellow().bold().to_string()
        } else {
            entry.subject.white().to_string()
        };

        let tag = if let Some(idx) = entry.bucket_idx {
            if idx < buckets.len() {
                format!("  {}", colorize_bucket_tag(&buckets[idx].name, idx))
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        lines.push(format!(
            "  {marker_str} {dot} {sha_str}  {subject_str}{tag}"
        ));
    }

    if view_end < total {
        lines.push(format!(
            "  {} more below",
            format!("↓ {}", total - view_end).dimmed()
        ));
    }

    lines.push(String::new());

    // Buckets summary
    lines.push(format!("  {}:", "Buckets".bold()));

    if buckets.is_empty() {
        lines.push(format!("  {}", "(none — press 'n' to create)".dimmed()));
    } else {
        for (i, bucket) in buckets.iter().enumerate() {
            let count = entries.iter().filter(|e| e.bucket_idx == Some(i)).count();
            let num = format!("[{}]", i + 1);
            lines.push(format!(
                "  {} {} ({} commit{})",
                num.dimmed(),
                colorize_bucket_name(&bucket.name, i),
                count,
                if count == 1 { "" } else { "s" }
            ));
        }
    }

    let unassigned = entries.iter().filter(|e| e.bucket_idx.is_none()).count();
    if unassigned > 0 {
        lines.push(format!(
            "      {} ({} commit{})",
            "unassigned".dimmed(),
            unassigned,
            if unassigned == 1 { "" } else { "s" }
        ));
    }

    lines.push(String::new());

    // Mode-specific footer
    match mode {
        Mode::Normal => {
            lines.push(format!(
                "  {}",
                "↑/↓ navigate │ 1-9 assign │ n new bucket │ u unassign │ Enter confirm │ q quit"
                    .dimmed()
            ));
        }
        Mode::TextInput => {
            lines.push(format!(
                "  {} {}▏",
                "Branch name:".bold(),
                input_buf
            ));
            lines.push(format!(
                "  {}",
                "Enter confirm │ Esc cancel".dimmed()
            ));
        }
    }

    // Error message
    if let Some(err) = input_error {
        lines.push(format!("  {}", err.red()));
    }

    *last_height = lines.len();
    for line in &lines {
        term.write_line(line)?;
    }

    Ok(())
}

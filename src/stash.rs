use anyhow::Result;

use crate::context::Ctx;
use crate::ui;

const STASH_PREFIX: &str = "gw:";

fn stash_message(branch: &str) -> String {
    format!("{STASH_PREFIX}{branch}")
}

/// Auto-stash dirty work on current branch, checkout target,
/// auto-unstash if target has a tagged stash.
///
/// If checkout fails after stashing, restores the stash before returning
/// the error (safety invariant: never lose uncommitted work).
///
/// During active propagation, skips auto-stash/unstash and falls back
/// to a plain checkout.
pub fn checkout_with_stash(ctx: &Ctx, target: &str) -> Result<()> {
    // Skip auto-stash during active propagation to avoid stashing
    // conflict markers or interfering with the rebase cascade.
    if ctx.propagation_state()?.is_some() {
        ctx.git.checkout(target)?;
        return Ok(());
    }

    let current = ctx.git.current_branch()?;
    let dirty = !ctx.git.is_working_tree_clean()?;

    // Stash if dirty
    if dirty {
        ctx.git.stash_push(&stash_message(&current))?;
        ui::info(&format!("Stashed changes for {current}"));
    }

    // Checkout with rollback on failure
    if let Err(e) = ctx.git.checkout(target) {
        if dirty {
            // Restore the stash we just pushed so the user doesn't lose work
            let _ = ctx.git.stash_pop(0);
        }
        return Err(e);
    }

    // Unstash if target has a tagged stash
    let matches = ctx.git.stash_list_matching(&stash_message(target))?;
    if !matches.is_empty() {
        let index = matches[0]; // Most recent match
        let had_conflicts = ctx.git.stash_pop(index)?;
        if had_conflicts {
            ui::warn(&format!(
                "Restored stashed changes for {target} (with conflicts)"
            ));
            ui::warn("Resolve conflict markers in your working tree.");
            ui::warn("The stash was not dropped. Run `git stash drop` after resolving.");
        } else {
            ui::info(&format!("Restored stashed changes for {target}"));
        }
        if matches.len() > 1 {
            ui::warn(&format!(
                "Note: {} additional gw stash(es) exist for {target}",
                matches.len() - 1
            ));
        }
    }

    Ok(())
}

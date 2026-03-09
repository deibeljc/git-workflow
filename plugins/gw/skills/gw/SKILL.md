---
name: gw
description: >
  Guides Claude to use gw (Git Workflow) for all branch and stack management
  instead of raw git commands. Automatically applies when doing git operations
  like committing, branching, pushing, creating PRs, or organizing work into
  reviewable chunks.
  TRIGGER when: user asks to commit, push, create a branch, create a PR, break
  work into pieces, rebase, sync with main, or any git workflow operation.
  DO NOT TRIGGER when: reading files, running tests, editing code with no git
  intent.
allowed-tools: Bash, Read, Edit, Write, Glob, Grep, mcp__gw__gw_stack_create, mcp__gw__gw_stack_list, mcp__gw__gw_stack_delete, mcp__gw__gw_branch_create, mcp__gw__gw_branch_remove, mcp__gw__gw_log, mcp__gw__gw_log_pr, mcp__gw__gw_push, mcp__gw__gw_rebase, mcp__gw__gw_rebase_continue, mcp__gw__gw_rebase_abort, mcp__gw__gw_sync, mcp__gw__gw_sync_rebase, mcp__gw__gw_sync_merged, mcp__gw__gw_switch, mcp__github__create_pull_request
---

# Git Workflow with gw

You are working in a repo that uses **gw**, a stacked branch manager. Use gw tools instead of raw git for all branch management, pushing, rebasing, and syncing operations. Raw git is still fine for commits, staging, diffs, and status.

## Core Principle

**gw manages branches. git manages commits.** Use git for committing and staging. Use gw for everything that involves branch relationships, pushing, rebasing, or syncing.

## When to Use What

### Use raw git for:
- `git add` / `git stage` - staging files
- `git commit` - creating commits
- `git status` - checking working tree state
- `git diff` - viewing changes
- `git log` - viewing commit history (though `gw_log` shows the stack view)
- `git stash` - stashing changes

### Use gw MCP tools for:
- **Creating branches** - `gw_branch_create` instead of `git checkout -b`
- **Switching branches** - `gw_switch` instead of `git checkout`
- **Pushing** - `gw_push` instead of `git push` (handles force-with-lease automatically)
- **Rebasing** - `gw_rebase` instead of `git rebase` (propagates to descendants)
- **Syncing with base** - `gw_sync` instead of `git pull --rebase`
- **Viewing branch state** - `gw_log` instead of `git branch`

## Workflow Patterns

### Starting new work

1. Check if there's an existing stack: `gw_log`
2. Create a new stack: `gw_stack_create` with a short descriptive name
3. Do work, commit with git as normal
4. When the first logical unit is done and you need to start the next piece, create the next branch: `gw_branch_create`

### Breaking work into stacked PRs

When the user has a large change or asks to split work into reviewable pieces:

**If the work hasn't been done yet:**
1. Create a stack with `gw_stack_create`
2. Make commits for the first logical unit (e.g., data model changes)
3. Create the next branch with `gw_branch_create` for the next unit (e.g., API layer)
4. Repeat until done
5. Each branch becomes its own PR, stacked on the previous one

**If the work is already on a single branch:**
1. Use `gw split` to decompose the existing branch into a clean stack
2. The interactive TUI lets you assign each commit to a named bucket/branch
3. Or use `gw split --plan <file>` with a plan file for scripting
4. Cherry-pick conflicts can be resolved with `gw split --continue`
5. Use `gw split --abort` to roll back if needed

Good stack boundaries: model/schema changes, API/service layer, UI/frontend, tests, migrations. Each branch should be independently reviewable and tell a coherent story.

### Pushing work

Use `gw_push` to push the current branch. It only pushes the current branch and automatically uses force-with-lease when needed (e.g., after a rebase). Never use raw `git push` for branches managed by gw.

### After making changes to an earlier branch

If you commit changes to a branch that has descendants:

1. Make your changes and commit with git
2. Run `gw_rebase` to propagate the rebase to all descendant branches

### Handling conflicts during rebase

If `gw_rebase` reports a conflict:

1. Check `git status` to see conflicted files
2. Read the conflicted files and resolve them
3. Stage resolved files with `git add`
4. Run `gw_rebase_continue` to resume the cascade
5. If things go wrong, `gw_rebase_abort` rolls back ALL branches

### Syncing after a PR is merged

When a PR gets squash-merged on GitHub:

1. Run `gw_sync` - it fetches the base branch, detects which branches were merged, removes them from the stack, and rebases the remaining branches
2. If you need to explicitly rebase onto the latest base (e.g., to pick up other people's changes), use `gw_sync_rebase`

### Creating PRs for a stack

Create PRs bottom-up (root branch first, then its children). Each PR targets its parent branch:

- Root branch PR targets the base branch (e.g., `main`)
- Child branch PRs target their parent branch in the stack

Use `gw_log_pr` to see PR status for all branches in the stack.

## Important Rules

1. **Never `git push` directly** for gw-managed branches. Always use `gw_push`.
2. **Never `git checkout -b`** to create branches that should be part of a stack. Use `gw_branch_create`.
3. **Never `git rebase` directly** on stacked branches. Use `gw_rebase` so descendants get updated.
4. **Check `gw_log` first** before creating new branches to understand the current stack state.
5. **One concern per branch** - each branch in a stack should have a clear, reviewable purpose.
6. **Commit with git, manage with gw** - this is the fundamental split.

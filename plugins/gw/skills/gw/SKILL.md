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
allowed-tools: Bash, Read, Edit, Write, Glob, Grep, mcp__gw__gw_stack_create, mcp__gw__gw_stack_list, mcp__gw__gw_stack_delete, mcp__gw__gw_branch_create, mcp__gw__gw_branch_remove, mcp__gw__gw_log, mcp__gw__gw_log_pr, mcp__gw__gw_push, mcp__gw__gw_force_push, mcp__gw__gw_rebase, mcp__gw__gw_rebase_continue, mcp__gw__gw_rebase_abort, mcp__gw__gw_sync, mcp__gw__gw_sync_rebase, mcp__gw__gw_sync_merged, mcp__gw__gw_switch, mcp__gw__gw_split, mcp__gw__gw_split_continue, mcp__gw__gw_split_abort, mcp__github__create_pull_request
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
- **Splitting branches** - `gw_split` to decompose a fat branch into a stack

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

**If the work is already on a single branch (use gw_split):**

This is the most common case when refactoring after the fact. Follow these steps precisely:

1. **Get the commits** on the current branch using `git log --format='%H %s' <base>..HEAD` where `<base>` is the branch you branched from (usually `main` or `dev`). You need the full 40-character SHAs.

2. **Design the split** by grouping commits into logical units. Good boundaries:
   - Model/schema changes
   - API/service layer
   - UI/frontend
   - Tests
   - Migrations
   Each group becomes a branch. Every commit must be assigned to exactly one branch.

3. **Build the plan** as a string with one line per commit:
   ```
   pick <full-sha> <branch-name>
   pick <full-sha> <branch-name>
   pick <full-sha> <other-branch>
   ```
   Commits assigned to the same branch-name are grouped together. The first unique branch name becomes the root of the stack, the second becomes its child, etc.

4. **Call `gw_split`** with the plan string and optionally a stack name.

5. **If conflicts occur**, resolve them:
   - Check `git status` to see conflicted files
   - Read and resolve the conflicts
   - Stage resolved files with `git add`
   - Call `gw_split_continue` to resume
   - If things go wrong, `gw_split_abort` rolls back everything

**Example split plan for a branch with 4 commits:**
```
pick a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0 feature-models
pick b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1 feature-models
pick c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2 feature-api
pick d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3 feature-tests
```
This creates a 3-branch stack: `feature-models` (2 commits) → `feature-api` (1 commit) → `feature-tests` (1 commit).

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
7. **Full SHAs in split plans** - `gw_split` requires 40-character commit SHAs, not short hashes.
8. **Every commit must be assigned** - a split plan must cover all commits on the branch and use at least 2 branch names.

# gw - Git Stacked Branch Manager

This repo contains `gw`, a CLI tool for managing stacked branches with GitHub's PR workflow.

## Key Concepts

- **Stack**: a named group of branches that form a parent-child chain off a base branch
- **Base branch**: the branch a stack is rooted on (e.g. `main`, `dev`)
- **Root branch**: the first branch in a stack (closest to base)
- **Leaf branch**: the last branch in a stack (furthest from base)
- Metadata lives in `.git/gw/` and is never pushed to remote

## Common Workflows

### Creating a stack and adding branches
```bash
gw stack create <name>          # creates stack + root branch, checks it out
# ... do work, commit ...
gw branch create <next-branch>  # adds child branch, checks it out
# ... do work, commit ...
gw branch create <another>      # keeps chaining
```

### Viewing stacks
```bash
gw tree                         # show all stacks with branches and commits
gw tree --pr                    # include PR status from GitHub (slower)
```

### After addressing PR feedback
```bash
git checkout <branch-with-feedback>
# ... make changes, commit ...
gw rebase                       # propagates rebase to all descendant branches
gw push --yes                   # push current branch (force-with-lease if needed)
```

### After a PR is squash-merged
```bash
gw sync                         # fetches base, detects merges, rebases remaining stack
```

### Switching branches
```bash
gw switch                       # interactive picker
gw switch <branch-name>         # direct checkout
```

### Adopting existing branches
```bash
gw adopt branch-a branch-b branch-c --base main
```

## Conflict Resolution

If `gw rebase` or `gw sync` hits a conflict:
```bash
# resolve conflicts in your editor
git add <resolved files>
gw rebase --continue            # resumes the cascade
# OR
gw rebase --abort               # rolls back ALL branches to pre-rebase state
```

Most commands are blocked during a propagation. Only `gw tree` and `gw switch` work.

## Important Notes

- `gw push` only pushes the current branch, never descendants
- `gw sync` only rebases when a branch was actually merged (stacks stay pinned otherwise)
- Branch names can't start with `-` (prevents git argument injection)
- Stack metadata is in `.git/gw/stacks/<name>.toml`, propagation state in `.git/gw/state.toml`

## Build & Test

```bash
cargo build                     # build
cargo test                      # run all tests
cargo install --path .          # install to ~/.cargo/bin/gw
```

## Architecture

- `src/cli.rs` - clap derive CLI definitions
- `src/commands/` - one file per command (stack, branch, adopt, rebase, sync, push, switch, tree, config, completions)
- `src/git.rs` - git CLI wrapper (all git operations go through here)
- `src/propagation.rs` - shared rebase cascade engine
- `src/state.rs` - TOML serialization for stack configs and propagation state
- `src/context.rs` - central context (Git instance, paths, stack loading)
- `src/gh.rs` - GitHub CLI integration (PR status, merge detection)
- `src/validate.rs` - input validation (branch names, stack names)
- `src/ui.rs` - terminal output helpers
- `tests/` - integration tests (one file per command area)

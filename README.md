# gw

A git stacked branch manager that works with GitHub's pull request workflow, not against it.

## What is this

If you've ever worked on a feature that's big enough to break into multiple PRs, you know the pain. You create a chain of branches, and every time you push a commit to address review feedback on the first branch, you have to manually rebase every branch that comes after it. When that first PR finally gets squash-merged into dev, you have to figure out which commit on dev corresponds to your branch, remove the branch from the chain, and rebase everything again. It's the grunt work of stacked branches, and it's the kind of thing a tool should handle for you.

`gw` tracks parent-child relationships between branches in a stack, automatically propagates rebases when you modify an upstream branch, and detects when branches get squash-merged so it can clean up the stack. All metadata lives in `.git/gw/` and never gets pushed to the remote. Your branches are real git branches, your PRs are normal GitHub PRs, and gw just handles the tedious coordination between them.

### Why build another stacking tool

There are a few stacking tools out there and they're all doing interesting things, but they each make tradeoffs that don't fit the workflow most teams on GitHub actually use.

**Graphite** requires a cloud account and wraps `git push` with its own CLI. Your branches get managed through their service and you need their dashboard to get the full picture. If you want something local-only that doesn't add a SaaS dependency to your git workflow, it's not the right fit.

**ghstack** rewrites your branches into a format that GitHub can display as individual PRs, but the branches it creates aren't real branches you'd work with normally. It's clever but it means your local branch structure doesn't match what's on GitHub, and that impedance mismatch gets confusing fast.

**git-branchless** is powerful but it's a fundamentally different mental model. It reimagines git around changes rather than branches, inspired by Mercurial and Phabricator. If you're deep in GitHub's PR workflow where one branch maps to one PR and you squash-merge into dev, that model doesn't map cleanly.

The idea behind `gw` is that it should play the nicest with the review workflow teams are already doing on GitHub. Each branch in a stack maps 1:1 to a PR. You push each branch individually when you're ready. You address PR feedback by committing to that branch and propagating the rebase. Your reviewer sees a normal PR with a normal diff. And when the PR gets squash-merged, gw detects it and cleans up the stack automatically. It's a thin wrapper around git, not a replacement for it.

## Install

### From source

```
git clone https://github.com/yourusername/gw.git
cd gw
cargo install --path .
```

You'll need a [Rust toolchain](https://rustup.rs/) installed. The binary ends up in `~/.cargo/bin/gw`.

### What you'll need

- **git** on your PATH (you definitely already have this)
- **gh** is optional but super helpful. It lets gw auto-detect squash merges and show PR status in `gw tree`. Without it, you can still manually tell gw when a branch has been merged.

## Getting started

If your repo uses a base branch other than `main` (like `dev` at Webflow, or `develop` at a lot of other places), tell gw about it up front:

```
gw config set-base dev
```

This gets stored in `.git/gw/config.toml` and applies to all new stacks in this repo. You can always override it per-stack with `--base` if you need to.

## Usage

### Create a stack

```
gw stack create auth
```

This creates a new branch called `auth` off your base branch and starts tracking it as a stack. You get checked out onto the new branch and you're ready to go.

### Add branches to the stack

Do some work on auth, commit it, and then when you're ready to start the next piece:

```
gw branch create auth-tests
```

This creates `auth-tests` as a child of `auth` and checks you out onto it. Keep going as deep as you want:

```
gw branch create auth-ui
```

Now you have a stack: `dev -> auth -> auth-tests -> auth-ui`

### See your stacks

```
gw tree
```

This shows all your stacks with their branches and commits underneath:

```
◇  dev
├─  ◆  auth  root
│   │ a1b2c3d implement auth middleware
│   │ e4f5g6h add token refresh flow
│
├─  @  auth-tests
│   │ i7j8k9l add auth unit tests
│
╰─  ◆  auth-ui
    │ m0n1o2p add login page component
```

`@` marks your current branch and `◆` marks the others. The commits under each branch are the ones that would show up in a PR for that branch.

### Address PR feedback and propagate

This is the meat and potatoes of the whole tool. Your `auth` branch has a PR open and reviewers left feedback. You check out auth, make your changes, commit, and then:

```
gw rebase
```

That's it. gw automatically rebases all the descendant branches (`auth-tests` and `auth-ui`) onto the updated `auth`. If there's a conflict somewhere in the chain, gw pauses and lets you resolve it:

```
# resolve the conflicts however you normally would
git add <resolved files>
gw rebase --continue
```

Or if the whole thing is a mess and you want to start over:

```
gw rebase --abort
```

This rolls every branch back to exactly where it was before you started the propagation.

### Push a branch

```
gw push
```

This pushes just the current branch. If the branch has diverged from the remote because of a rebase, gw asks before force-pushing with lease. It never touches descendant branches, so you push each one individually when you're ready for review.

### Sync after a merge

When `auth` gets squash-merged into dev, run:

```
gw sync
```

This pulls dev, detects that `auth` was merged (using the `gh` CLI to check PR status, or by comparing the git trees), removes it from the stack, and rebases everything that's left onto the updated dev. The next branch in the chain becomes the new root.

If you don't have `gh` installed or it can't detect the merge automatically, you can just tell gw:

```
gw sync --merged auth
```

### Adopt existing branches

Already have a bunch of branches you want to turn into a stack? You can adopt them:

```
gw adopt feature-a feature-b feature-c --base dev
```

The order of the arguments defines the stack order, so `feature-a` becomes the root and `feature-c` becomes the leaf. If the branches aren't already chained together, gw rebases them into a proper chain and asks for confirmation first.

### Remove a branch from a stack

```
gw branch remove auth-tests
```

If auth-tests had children, they get re-parented onto auth-tests's parent and rebased accordingly. The git branch itself doesn't get deleted, it just gets untracked by gw.

### Delete a stack

```
gw stack delete auth
```

This removes gw's tracking metadata and nothing else. All the git branches stay right where they are.

## How it works under the hood

gw stores stack metadata as TOML files in `.git/gw/stacks/`. Each stack gets a file like `.git/gw/stacks/auth.toml` that looks like this:

```toml
name = "auth"
base_branch = "dev"

[[branches]]
name = "auth"

[[branches]]
name = "auth-tests"

[[branches]]
name = "auth-ui"
```

The array order defines the stack. First branch is the root (closest to base) and the last is the leaf.

During rebase propagation, gw writes state to `.git/gw/state.toml` so it can resume after conflicts or roll back on abort. All other gw commands get blocked until the propagation is resolved, so you can't accidentally corrupt your stack state.

Nothing in `.git/gw/` ever gets pushed to the remote. It's purely local tracking.

## All commands

| Command | What it does |
|---------|-------------|
| `gw stack create <name>` | Create a new stack off the base branch |
| `gw stack delete <name>` | Remove stack metadata (branches stay) |
| `gw stack list` | List all stacks |
| `gw branch create <name>` | Add a branch to the current stack |
| `gw branch remove <name>` | Remove a branch and re-parent children |
| `gw adopt <branches...>` | Adopt existing branches into a stack |
| `gw rebase` | Propagate rebases to descendants |
| `gw rebase --continue` | Resume after resolving conflicts |
| `gw rebase --abort` | Roll back all branches |
| `gw sync` | Pull base, detect merges, rebase stack |
| `gw sync --merged <branch>` | Manually indicate a branch was merged |
| `gw push` | Push the current branch |
| `gw tree` | Show all stacks with branches and commits |
| `gw config set-base <branch>` | Set the default base branch |
| `gw config show` | Show current configuration |

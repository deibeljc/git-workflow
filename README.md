# gw

You break a big feature into multiple PRs. You push the first one for review, get feedback, commit a fix, and now you have to manually rebase every branch after it. When that PR finally gets squash-merged, you have to figure out which commit on dev matches your branch, remove it from the chain, and rebase everything again. Over and over.

`gw` handles all of that. It tracks the parent-child relationships between your branches, automatically propagates rebases through the chain, and detects squash merges so it can clean up the stack. Your branches are real git branches, your PRs are normal GitHub PRs, and gw just does the tedious coordination between them.

Everything lives in `.git/gw/` and never gets pushed to the remote.

<p align="center">
  <img src="docs/gw-tree.png" alt="gw log output showing three stacks with branches and commits" width="640">
</p>

## Why this exists

The core problem is that GitHub's PR workflow assumes branches are independent. When you stack them, you're fighting the tool. Every commit to an upstream branch means manually rebasing everything downstream. Every squash merge means figuring out what landed, removing the merged branch, and rebasing again. The overhead scales with the number of branches in the chain, and it gets painful fast.

There's also a subtler problem that most stacking tools ignore: force pushing. When a tool automatically rebases your stack onto the latest base branch, every branch in the chain gets new SHAs and needs a force push. GitHub treats force pushes poorly in reviews. Your reviewer's "viewed" state resets, inline comments get orphaned, and the diff becomes harder to follow because GitHub can't cleanly track what changed between the old and new versions of a force-pushed branch. If you're stacking PRs to make review easier, a tool that force pushes your whole stack every time someone merges to main is actively working against that goal.

Existing tools solve the rebase automation but they all ask you to give something up.

**Graphite** is a full platform. It requires a cloud account, wraps your push workflow through their service, and adds a dashboard on top of GitHub. If you're on a team that's bought in, it works great. But if you just want the rebase automation without adding a SaaS dependency to your git workflow, it's a lot of overhead for what should be a local operation.

**ghstack** takes a fundamentally different approach where it rewrites your branches into a format that GitHub can display as individual PRs. The tradeoff is that what's on your machine doesn't match what's on GitHub. Your local branch has all the commits, but the PR shows a synthetic branch with just the relevant ones. That impedance mismatch gets confusing when you're trying to debug why a rebase went wrong or why a PR diff looks different than what you expect locally.

**git-branchless** is powerful but it's a different mental model entirely. It's inspired by Mercurial and Phabricator, and it reimagines git around changes rather than branches. If your team already does one-branch-per-PR and squash-merges into a base branch, that abstraction doesn't map cleanly to your existing workflow, and you're learning a new way of thinking about git on top of learning the tool.

`gw` is intentionally boring. Your branches are real git branches. Your PRs are normal GitHub PRs. Your reviewers see normal diffs. Nothing gets rewritten, nothing gets synced to a cloud service, and the mental model is the same one you already have. It just automates the tedious parts: propagating rebases through the chain, detecting squash merges, and cleaning up the stack when branches land.

Critically, `gw sync` does not rebase your stack onto the latest base branch unless a branch was actually merged or you explicitly ask for it with `--rebase`. Your stack stays pinned to the base commit it started from, which means your open PRs don't get force pushed just because someone else merged to main. When you do need to update, you control when that happens.

## Install

```
git clone https://github.com/deibeljc/git-workflow.git
cd git-workflow
cargo install --path .
```

Needs a [Rust toolchain](https://rustup.rs/). Optional: `gh` CLI for auto-detecting squash merges and showing PR status in `gw log`.

### Shell completions

Tab completion for commands, flags, and branch names:

```bash
# zsh (add to ~/.zshrc)
source <(gw completions zsh)

# bash (add to ~/.bashrc)
eval "$(gw completions bash)"

# fish
gw completions fish | source
```

### Claude Code integration

`gw` has a built-in MCP server so Claude Code can manage your stacks directly. Set it up with one command:

```bash
gw mcp-setup
```

This writes the MCP config to `.mcp.json` and you're good to go. Restart Claude Code and it can use gw tools natively.

## Quick start

```bash
# Set your base branch (if not main)
gw config set-base dev

# Create a stack
gw stack create auth

# Do work, commit, then add the next branch
gw branch create auth-tests

# See everything (just `gw` also works)
gw log

# Address PR feedback on auth, then propagate rebases
gw rebase

# Push when ready
gw push

# After auth gets squash-merged
gw sync

# Explicitly rebase onto latest base when you're ready
gw sync --rebase

# Switch between branches interactively
gw switch
```

## Already have branches? Adopt them

If you've already got a chain of branches you've been managing by hand, you don't need to recreate anything. Just tell gw about them:

```bash
gw adopt feature-api feature-tests feature-ui --base dev
```

The argument order defines the stack order, so `feature-api` becomes the root and `feature-ui` becomes the leaf. If the branches aren't already rebased into a chain, gw handles that for you and asks before making changes. You can also name the stack explicitly with `--name` or let it default to the first branch name.

This is the easiest way to migrate onto gw. You keep all your existing branches and commits, gw just starts tracking the relationships between them.

## Commands

| Command | What it does |
| --- | --- |
| `gw` | Show all stacks (alias for `gw log`) |
| `gw log` | Show all stacks with branches and commits |
| `gw log --pr` | Include PR status from GitHub |
| `gw stack create <name>` | Create a new stack off the base branch |
| `gw stack delete <name>` | Remove stack metadata (branches stay) |
| `gw stack list` | List all stacks |
| `gw branch create <name>` | Add a branch to the current stack |
| `gw branch remove <name>` | Remove a branch and re-parent children |
| `gw adopt <branches...>` | Adopt existing branches into a stack |
| `gw rebase` | Propagate rebases to descendants |
| `gw rebase --continue` | Resume after resolving conflicts |
| `gw rebase --abort` | Roll back all branches |
| `gw sync` | Fetch base, detect merges, rebase stack |
| `gw sync --rebase` | Explicitly rebase stack onto latest base |
| `gw sync --merged <branch>` | Manually indicate a branch was merged |
| `gw push` | Push the current branch |
| `gw switch [branch]` | Switch branches interactively or by name |
| `gw config set-base <branch>` | Set the default base branch |
| `gw config show` | Show current configuration |
| `gw completions <shell>` | Generate shell completions (zsh/bash/fish) |

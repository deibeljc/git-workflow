use anyhow::{bail, Result};

pub fn run(shell: &str) -> Result<()> {
    match shell {
        "zsh" => print!("{}", ZSH_COMPLETIONS),
        "bash" => print!("{}", BASH_COMPLETIONS),
        "fish" => print!("{}", FISH_COMPLETIONS),
        _ => bail!("Unsupported shell: {shell}. Use zsh, bash, or fish."),
    }
    Ok(())
}

const ZSH_COMPLETIONS: &str = r#"#compdef gw

_gw_branches() {
  local -a branches
  branches=(${(f)"$(git branch --format='%(refname:short)' 2>/dev/null)"})
  _describe 'branch' branches
}

_gw_stacks() {
  local -a stacks
  local gw_dir="$(git rev-parse --git-dir 2>/dev/null)/gw/stacks"
  if [[ -d "$gw_dir" ]]; then
    stacks=(${(f)"$(ls "$gw_dir" 2>/dev/null | sed 's/\.toml$//')"})
  fi
  _describe 'stack' stacks
}

_gw() {
  local -a commands
  commands=(
    'stack:Manage stacks'
    'branch:Manage branches within a stack'
    'adopt:Adopt existing branches into a stack'
    'rebase:Propagate rebases to descendant branches'
    'sync:Sync stacks with the base branch'
    'push:Push the current branch'
    'switch:Switch to a branch tracked by gw'
    'log:Show log of all stacks with branches and commits'
    'tree:Alias for log'
    'config:Configure gw settings'
    'completions:Generate shell completions'
  )

  _arguments -C \
    '--version[Print version]' \
    '--help[Print help]' \
    '1:command:->command' \
    '*::arg:->args'

  case $state in
    command)
      _describe 'command' commands
      ;;
    args)
      case $words[1] in
        stack)
          local -a stack_commands
          stack_commands=(
            'create:Create a new stack'
            'delete:Delete a stack'
            'list:List all stacks'
          )
          _arguments -C \
            '1:subcommand:->subcmd' \
            '*::arg:->stack_args'
          case $state in
            subcmd) _describe 'subcommand' stack_commands ;;
            stack_args)
              case $words[1] in
                create) _arguments '1:name:' '--base[Base branch]:branch:_gw_branches' ;;
                delete) _arguments '1:name:_gw_stacks' ;;
              esac
              ;;
          esac
          ;;
        branch)
          local -a branch_commands
          branch_commands=(
            'create:Create a new branch in the current stack'
            'remove:Remove a branch from its stack'
          )
          _arguments -C \
            '1:subcommand:->subcmd' \
            '*::arg:->branch_args'
          case $state in
            subcmd) _describe 'subcommand' branch_commands ;;
            branch_args)
              case $words[1] in
                create) _arguments '1:name:' ;;
                remove) _arguments '1:name:_gw_branches' ;;
              esac
              ;;
          esac
          ;;
        adopt)
          _arguments \
            '*:branch:_gw_branches' \
            '--base[Base branch]:branch:_gw_branches' \
            '--name[Stack name]:name:' \
            '--yes[Skip confirmation]'
          ;;
        rebase)
          _arguments \
            '--continue[Continue after resolving conflicts]' \
            '--abort[Abort and roll back]'
          ;;
        sync)
          _arguments \
            '--stack[Sync specific stack]:stack:_gw_stacks' \
            '--merged[Branch that was merged]:branch:_gw_branches' \
            '--rebase[Rebase stack onto latest base]'
          ;;
        push)
          _arguments '--yes[Skip confirmation]'
          ;;
        switch)
          _arguments '1:branch:_gw_branches'
          ;;
        log|tree)
          _arguments '--pr[Show PR status]'
          ;;
        config)
          local -a config_commands
          config_commands=(
            'set-base:Set the default base branch'
            'show:Show current configuration'
          )
          _arguments -C \
            '1:subcommand:->subcmd' \
            '*::arg:->config_args'
          case $state in
            subcmd) _describe 'subcommand' config_commands ;;
            config_args)
              case $words[1] in
                set-base) _arguments '1:branch:_gw_branches' ;;
              esac
              ;;
          esac
          ;;
        completions)
          _arguments '1:shell:(zsh bash fish)'
          ;;
      esac
      ;;
  esac
}

compdef _gw gw
"#;

const BASH_COMPLETIONS: &str = r#"_gw_branches() {
  git branch --format='%(refname:short)' 2>/dev/null
}

_gw_stacks() {
  local gw_dir
  gw_dir="$(git rev-parse --git-dir 2>/dev/null)/gw/stacks"
  if [[ -d "$gw_dir" ]]; then
    ls "$gw_dir" 2>/dev/null | sed 's/\.toml$//'
  fi
}

_gw() {
  local cur prev words cword
  _init_completion || return

  local commands="stack branch adopt rebase sync push switch log tree config completions"
  local stack_commands="create delete list"
  local branch_commands="create remove"
  local config_commands="set-base show"

  if [[ $cword -eq 1 ]]; then
    COMPREPLY=($(compgen -W "$commands" -- "$cur"))
    return
  fi

  case "${words[1]}" in
    stack)
      if [[ $cword -eq 2 ]]; then
        COMPREPLY=($(compgen -W "$stack_commands" -- "$cur"))
      elif [[ $cword -ge 3 ]]; then
        case "${words[2]}" in
          delete) COMPREPLY=($(compgen -W "$(_gw_stacks)" -- "$cur")) ;;
          create)
            if [[ "$prev" == "--base" ]]; then
              COMPREPLY=($(compgen -W "$(_gw_branches)" -- "$cur"))
            else
              COMPREPLY=($(compgen -W "--base" -- "$cur"))
            fi
            ;;
        esac
      fi
      ;;
    branch)
      if [[ $cword -eq 2 ]]; then
        COMPREPLY=($(compgen -W "$branch_commands" -- "$cur"))
      elif [[ $cword -ge 3 ]]; then
        case "${words[2]}" in
          remove) COMPREPLY=($(compgen -W "$(_gw_branches)" -- "$cur")) ;;
        esac
      fi
      ;;
    adopt)
      if [[ "$prev" == "--base" || "$prev" == "--name" ]]; then
        [[ "$prev" == "--base" ]] && COMPREPLY=($(compgen -W "$(_gw_branches)" -- "$cur"))
      else
        COMPREPLY=($(compgen -W "$(_gw_branches) --base --name --yes" -- "$cur"))
      fi
      ;;
    rebase)
      COMPREPLY=($(compgen -W "--continue --abort" -- "$cur"))
      ;;
    sync)
      if [[ "$prev" == "--stack" ]]; then
        COMPREPLY=($(compgen -W "$(_gw_stacks)" -- "$cur"))
      elif [[ "$prev" == "--merged" ]]; then
        COMPREPLY=($(compgen -W "$(_gw_branches)" -- "$cur"))
      else
        COMPREPLY=($(compgen -W "--stack --merged --rebase" -- "$cur"))
      fi
      ;;
    push)
      COMPREPLY=($(compgen -W "--yes" -- "$cur"))
      ;;
    switch)
      COMPREPLY=($(compgen -W "$(_gw_branches)" -- "$cur"))
      ;;
    tree)
      COMPREPLY=($(compgen -W "--pr" -- "$cur"))
      ;;
    config)
      if [[ $cword -eq 2 ]]; then
        COMPREPLY=($(compgen -W "$config_commands" -- "$cur"))
      elif [[ "${words[2]}" == "set-base" ]]; then
        COMPREPLY=($(compgen -W "$(_gw_branches)" -- "$cur"))
      fi
      ;;
    completions)
      COMPREPLY=($(compgen -W "zsh bash fish" -- "$cur"))
      ;;
  esac
}

complete -F _gw gw
"#;

const FISH_COMPLETIONS: &str = r#"function __gw_branches
  git branch --format='%(refname:short)' 2>/dev/null
end

function __gw_stacks
  set -l gw_dir (git rev-parse --git-dir 2>/dev/null)/gw/stacks
  if test -d "$gw_dir"
    ls "$gw_dir" 2>/dev/null | string replace -r '\.toml$' ''
  end
end

function __gw_needs_command
  set -l cmd (commandline -opc)
  test (count $cmd) -eq 1
end

function __gw_using_command
  set -l cmd (commandline -opc)
  test (count $cmd) -gt 1; and test "$cmd[2]" = "$argv[1]"
end

function __gw_using_subcommand
  set -l cmd (commandline -opc)
  test (count $cmd) -gt 2; and test "$cmd[2]" = "$argv[1]"; and test "$cmd[3]" = "$argv[2]"
end

# Top-level commands
complete -c gw -f -n __gw_needs_command -a stack -d 'Manage stacks'
complete -c gw -f -n __gw_needs_command -a branch -d 'Manage branches'
complete -c gw -f -n __gw_needs_command -a adopt -d 'Adopt existing branches'
complete -c gw -f -n __gw_needs_command -a rebase -d 'Propagate rebases'
complete -c gw -f -n __gw_needs_command -a sync -d 'Sync with base branch'
complete -c gw -f -n __gw_needs_command -a push -d 'Push current branch'
complete -c gw -f -n __gw_needs_command -a switch -d 'Switch branches'
complete -c gw -f -n __gw_needs_command -a log -d 'Show stacks'
complete -c gw -f -n __gw_needs_command -a tree -d 'Alias for log'
complete -c gw -f -n __gw_needs_command -a config -d 'Configure settings'
complete -c gw -f -n __gw_needs_command -a completions -d 'Generate completions'

# stack subcommands
complete -c gw -f -n '__gw_using_command stack' -a create -d 'Create a new stack'
complete -c gw -f -n '__gw_using_command stack' -a delete -d 'Delete a stack'
complete -c gw -f -n '__gw_using_command stack' -a list -d 'List all stacks'
complete -c gw -f -n '__gw_using_subcommand stack delete' -a '(__gw_stacks)'
complete -c gw -f -n '__gw_using_subcommand stack create' -l base -d 'Base branch' -ra '(__gw_branches)'

# branch subcommands
complete -c gw -f -n '__gw_using_command branch' -a create -d 'Create branch'
complete -c gw -f -n '__gw_using_command branch' -a remove -d 'Remove branch'
complete -c gw -f -n '__gw_using_subcommand branch remove' -a '(__gw_branches)'

# adopt
complete -c gw -f -n '__gw_using_command adopt' -a '(__gw_branches)'
complete -c gw -f -n '__gw_using_command adopt' -l base -d 'Base branch' -ra '(__gw_branches)'
complete -c gw -f -n '__gw_using_command adopt' -l name -d 'Stack name'
complete -c gw -f -n '__gw_using_command adopt' -l yes -d 'Skip confirmation'

# rebase
complete -c gw -f -n '__gw_using_command rebase' -l continue -d 'Continue after conflicts'
complete -c gw -f -n '__gw_using_command rebase' -l abort -d 'Abort and roll back'

# sync
complete -c gw -f -n '__gw_using_command sync' -l stack -d 'Sync specific stack' -ra '(__gw_stacks)'
complete -c gw -f -n '__gw_using_command sync' -l merged -d 'Branch that was merged' -ra '(__gw_branches)'
complete -c gw -f -n '__gw_using_command sync' -l rebase -d 'Rebase stack onto latest base'

# push
complete -c gw -f -n '__gw_using_command push' -l yes -d 'Skip confirmation'

# switch
complete -c gw -f -n '__gw_using_command switch' -a '(__gw_branches)'

# tree
complete -c gw -f -n '__gw_using_command log' -l pr -d 'Show PR status'
complete -c gw -f -n '__gw_using_command tree' -l pr -d 'Show PR status'

# config
complete -c gw -f -n '__gw_using_command config' -a set-base -d 'Set default base branch'
complete -c gw -f -n '__gw_using_command config' -a show -d 'Show configuration'
complete -c gw -f -n '__gw_using_subcommand config set-base' -a '(__gw_branches)'

# completions
complete -c gw -f -n '__gw_using_command completions' -a 'zsh bash fish'
"#;

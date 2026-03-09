use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::git::Git;
use crate::state::{self, ActiveState, GwConfig, PropagationState, SplitState, StackConfig};

/// Central context passed to all command handlers.
/// Bundles the Git instance, resolved gw paths, and provides
/// methods for loading/saving stack metadata.
pub struct Ctx {
    pub git: Git,
    pub gw_dir: PathBuf,
    pub stacks_dir: PathBuf,
    state_path: PathBuf,
    config_path: PathBuf,
}

impl Ctx {
    /// Discover the git repo and set up gw paths.
    pub fn discover() -> Result<Self> {
        let git = Git::discover()?;
        let gw_dir = git.repo_path().join(".git").join("gw");
        let stacks_dir = gw_dir.join("stacks");
        let state_path = gw_dir.join("state.toml");
        let config_path = gw_dir.join("config.toml");
        Ok(Self {
            git,
            gw_dir,
            stacks_dir,
            state_path,
            config_path,
        })
    }

    /// Bail if the working tree has uncommitted changes.
    pub fn require_clean_tree(&self) -> Result<()> {
        if !self.git.is_working_tree_clean()? {
            anyhow::bail!("You have uncommitted changes. Commit or stash before running this command.");
        }
        Ok(())
    }

    /// Ensure .git/gw/ and .git/gw/stacks/ directories exist.
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.stacks_dir)
            .with_context(|| format!("failed to create {}", self.stacks_dir.display()))
    }

    /// Load a stack by name.
    pub fn load_stack(&self, name: &str) -> Result<StackConfig> {
        let path = self.stacks_dir.join(format!("{name}.toml"));
        state::load_stack(&path)
    }

    /// Save a stack config.
    /// Path traversal is prevented by validate_stack_name() which rejects '/', '\', and '..'.
    pub fn save_stack(&self, config: &StackConfig) -> Result<()> {
        self.ensure_dirs()?;
        let path = self.stacks_dir.join(format!("{}.toml", config.name));

        state::save_stack(&path, config)
    }

    /// Delete a stack's TOML file.
    pub fn delete_stack(&self, name: &str) -> Result<()> {
        crate::validate::validate_stack_name(name)?;
        let path = self.stacks_dir.join(format!("{name}.toml"));
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        Ok(())
    }

    /// Check if a stack exists.
    pub fn stack_exists(&self, name: &str) -> bool {
        self.stacks_dir.join(format!("{name}.toml")).exists()
    }

    /// Load all stacks from .git/gw/stacks/*.toml
    pub fn load_all_stacks(&self) -> Result<Vec<StackConfig>> {
        if !self.stacks_dir.exists() {
            return Ok(vec![]);
        }

        let mut stacks = Vec::new();
        let entries = std::fs::read_dir(&self.stacks_dir)
            .with_context(|| format!("failed to read {}", self.stacks_dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                match state::load_stack(&path) {
                    Ok(config) => stacks.push(config),
                    Err(e) => {
                        eprintln!(
                            "Warning: failed to load {}: {e}",
                            path.display()
                        );
                    }
                }
            }
        }

        Ok(stacks)
    }

    /// Find which stack contains the given branch name.
    pub fn find_stack_for_branch(&self, branch: &str) -> Result<Option<StackConfig>> {
        let stacks = self.load_all_stacks()?;
        Ok(stacks
            .into_iter()
            .find(|s| s.branch_index(branch).is_some()))
    }

    /// Load propagation state if it exists.
    pub fn propagation_state(&self) -> Result<Option<PropagationState>> {
        state::load_propagation_state(&self.state_path)
    }

    /// Save propagation state.
    pub fn save_propagation_state(&self, ps: &PropagationState) -> Result<()> {
        self.ensure_dirs()?;
        state::save_propagation_state(&self.state_path, ps)
    }

    /// Remove propagation state.
    pub fn remove_propagation_state(&self) -> Result<()> {
        state::remove_propagation_state(&self.state_path)
    }

    /// Load active state (either propagation or split) if it exists.
    pub fn active_state(&self) -> Result<Option<ActiveState>> {
        state::load_active_state(&self.state_path)
    }

    /// Save split state.
    pub fn save_split_state(&self, ss: &SplitState) -> Result<()> {
        self.ensure_dirs()?;
        state::save_split_state(&self.state_path, ss)
    }

    /// Remove state file (works for both propagation and split state).
    pub fn remove_state(&self) -> Result<()> {
        state::remove_state(&self.state_path)
    }

    /// Load gw config (returns defaults if no config file exists).
    pub fn load_config(&self) -> Result<GwConfig> {
        state::load_config(&self.config_path)
    }

    /// Save gw config.
    pub fn save_config(&self, config: &GwConfig) -> Result<()> {
        self.ensure_dirs()?;
        state::save_config(&self.config_path, config)
    }

    /// Get the default base branch from config, or infer from the repo.
    pub fn default_base_branch(&self) -> Result<String> {
        let config = self.load_config()?;
        if let Some(ref base) = config.default_base {
            return Ok(base.clone());
        }
        // Infer: check for common base branch names
        let branches = self.git.all_local_branches()?;
        for candidate in &["dev", "develop", "main", "master"] {
            if branches.contains(*candidate) {
                return Ok(candidate.to_string());
            }
        }
        Ok(self.git.current_branch()?)
    }
}

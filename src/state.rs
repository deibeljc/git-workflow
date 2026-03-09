use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::validate;

/// Global gw configuration stored in .git/gw/config.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GwConfig {
    /// Default base branch for new stacks (e.g., "dev", "main")
    pub default_base: Option<String>,
    /// Whether to delete local branches after sync detects they were merged.
    /// Defaults to true when not set.
    pub delete_on_merge: Option<bool>,
}

impl GwConfig {
    pub fn should_delete_on_merge(&self) -> bool {
        self.delete_on_merge.unwrap_or(false)
    }
}

/// Load gw config from a TOML file. Returns default if file doesn't exist.
pub fn load_config(path: &Path) -> Result<GwConfig> {
    if !path.exists() {
        return Ok(GwConfig::default());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config: GwConfig =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(config)
}

/// Save gw config atomically.
pub fn save_config(path: &Path, config: &GwConfig) -> Result<()> {
    let content = toml::to_string_pretty(config).context("failed to serialize config")?;
    atomic_write(path, &content)
}

/// A single branch entry in a stack. Order in the Vec defines stack position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BranchEntry {
    pub name: String,
}

/// Stack metadata stored in .git/gw/stacks/<name>.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackConfig {
    pub name: String,
    pub base_branch: String,
    /// Ordered list of branches. First = root (closest to base), last = leaf.
    pub branches: Vec<BranchEntry>,
}

impl StackConfig {
    /// Find the index of a branch by name.
    pub fn branch_index(&self, name: &str) -> Option<usize> {
        self.branches.iter().position(|b| b.name == name)
    }

    /// Get the root branch (closest to base).
    pub fn root_branch(&self) -> Option<&BranchEntry> {
        self.branches.first()
    }

    /// Get the leaf branch (furthest from base).
    pub fn leaf_branch(&self) -> Option<&BranchEntry> {
        self.branches.last()
    }

    /// Get the parent branch name for a given branch.
    /// For the root branch, returns the base_branch.
    pub fn parent_of(&self, name: &str) -> Option<String> {
        let idx = self.branch_index(name)?;
        if idx == 0 {
            Some(self.base_branch.clone())
        } else {
            Some(self.branches[idx - 1].name.clone())
        }
    }

    /// Get all descendant branches (everything after the given branch in order).
    pub fn descendants_of(&self, name: &str) -> Vec<&BranchEntry> {
        if let Some(idx) = self.branch_index(name) {
            self.branches[idx + 1..].iter().collect()
        } else {
            vec![]
        }
    }

    /// Validate all branch names in the config.
    pub fn validate(&self) -> Result<()> {
        validate::validate_stack_name(&self.name)?;
        validate::validate_branch_name(&self.base_branch)?;
        for branch in &self.branches {
            validate::validate_branch_name(&branch.name)?;
        }
        Ok(())
    }
}

/// A ref snapshot for rollback purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginalRef {
    pub branch: String,
    pub commit: String,
}

/// The type of operation that created this propagation state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Rebase,
    Sync,
    Adopt,
    BranchRemove,
    Split,
}

/// Propagation state tracked in .git/gw/state.toml during multi-branch rebases.
/// Only exists while a propagation is in progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropagationState {
    pub operation: Operation,
    pub stack: String,
    pub started_at: String,
    /// The branch the user was on before propagation started.
    pub original_branch: String,
    /// Pre-operation refs for full rollback.
    pub original_refs: Vec<OriginalRef>,
    /// Branches that have been successfully rebased.
    pub completed: Vec<String>,
    /// Branches still to be rebased.
    pub remaining: Vec<String>,
    /// The branch currently being rebased (or that hit a conflict).
    pub current: Option<String>,
}

/// Write content to a file atomically using temp file + rename.
pub fn atomic_write(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .context("file path has no parent directory")?;
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create directory {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .context("failed to create temp file")?;
    tmp.write_all(content.as_bytes())
        .context("failed to write to temp file")?;
    tmp.persist(path)
        .with_context(|| format!("failed to persist temp file to {}", path.display()))?;
    Ok(())
}

/// Load a stack config from a TOML file.
pub fn load_stack(path: &Path) -> Result<StackConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config: StackConfig =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    // Validate stored values (defense in depth: files could be hand-edited)
    config.validate()?;
    Ok(config)
}

/// Save a stack config to a TOML file atomically.
pub fn save_stack(path: &Path, config: &StackConfig) -> Result<()> {
    let content =
        toml::to_string_pretty(config).context("failed to serialize stack config")?;
    atomic_write(path, &content)
}

/// Load propagation state from state.toml.
pub fn load_propagation_state(path: &Path) -> Result<Option<PropagationState>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let state: PropagationState = toml::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(state))
}

/// Save propagation state atomically.
pub fn save_propagation_state(path: &Path, state: &PropagationState) -> Result<()> {
    let content =
        toml::to_string_pretty(state).context("failed to serialize propagation state")?;
    atomic_write(path, &content)
}

/// Remove propagation state file.
pub fn remove_propagation_state(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

// ============================================================
// Split state
// ============================================================

/// A bucket of commits that will become a branch during split.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitBucket {
    pub name: String,
    pub commits: Vec<String>,
}

/// State tracked in .git/gw/state.toml during the cherry-pick phase of a split.
/// Only exists while a split's cherry-pick phase is in progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitState {
    pub operation: Operation,
    pub original_branch: String,
    pub original_sha: String,
    pub base_branch: String,
    pub stack_name: String,
    /// Branches created so far (for rollback on abort).
    pub created_branches: Vec<String>,
    /// Which bucket is currently being processed.
    pub current_bucket_index: usize,
    /// Which commit within the current bucket is next.
    pub current_commit_index: usize,
    /// The bucket assignments.
    pub buckets: Vec<SplitBucket>,
}

impl SplitState {
    /// Validate all fields after deserialization (defense in depth).
    pub fn validate(&self) -> Result<()> {
        crate::validate::validate_branch_name(&self.original_branch)?;
        crate::validate::validate_sha(&self.original_sha)?;
        crate::validate::validate_branch_name(&self.base_branch)?;
        crate::validate::validate_stack_name(&self.stack_name)?;
        for branch in &self.created_branches {
            crate::validate::validate_branch_name(branch)?;
        }
        for bucket in &self.buckets {
            crate::validate::validate_branch_name(&bucket.name)?;
            for sha in &bucket.commits {
                crate::validate::validate_sha(sha)?;
            }
        }
        Ok(())
    }
}

/// Discriminated union for state.toml which can contain either a PropagationState
/// or a SplitState depending on the active operation.
pub enum ActiveState {
    Propagation(PropagationState),
    Split(SplitState),
}

/// Load the active state from state.toml, determining the type from the content.
pub fn load_active_state(path: &Path) -> Result<Option<ActiveState>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    // Parse as raw TOML to check the operation and presence of split-specific fields
    let raw: toml::Value =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;

    let is_split = raw.get("operation").and_then(|v| v.as_str()) == Some("split")
        && raw.get("buckets").is_some();

    if is_split {
        let state: SplitState = toml::from_str(&content)
            .with_context(|| format!("failed to parse split state from {}", path.display()))?;
        state.validate()?;
        Ok(Some(ActiveState::Split(state)))
    } else {
        let state: PropagationState = toml::from_str(&content)
            .with_context(|| format!("failed to parse propagation state from {}", path.display()))?;
        Ok(Some(ActiveState::Propagation(state)))
    }
}

/// Save split state atomically.
pub fn save_split_state(path: &Path, state: &SplitState) -> Result<()> {
    let content =
        toml::to_string_pretty(state).context("failed to serialize split state")?;
    atomic_write(path, &content)
}

/// Remove state file (works for both split and propagation state).
pub fn remove_state(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_config_serialization() {
        let config = StackConfig {
            name: "auth".to_string(),
            base_branch: "dev".to_string(),
            branches: vec![
                BranchEntry {
                    name: "feature/auth".to_string(),
                },
                BranchEntry {
                    name: "feature/auth-tests".to_string(),
                },
            ],
        };

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: StackConfig = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.name, "auth");
        assert_eq!(deserialized.base_branch, "dev");
        assert_eq!(deserialized.branches.len(), 2);
        assert_eq!(deserialized.branches[0].name, "feature/auth");
        assert_eq!(deserialized.branches[1].name, "feature/auth-tests");
    }

    #[test]
    fn test_stack_navigation() {
        let config = StackConfig {
            name: "auth".to_string(),
            base_branch: "dev".to_string(),
            branches: vec![
                BranchEntry {
                    name: "auth".to_string(),
                },
                BranchEntry {
                    name: "auth-tests".to_string(),
                },
                BranchEntry {
                    name: "auth-ui".to_string(),
                },
            ],
        };

        assert_eq!(config.root_branch().unwrap().name, "auth");
        assert_eq!(config.leaf_branch().unwrap().name, "auth-ui");
        assert_eq!(config.parent_of("auth").unwrap(), "dev");
        assert_eq!(config.parent_of("auth-tests").unwrap(), "auth");
        assert_eq!(config.parent_of("auth-ui").unwrap(), "auth-tests");
        assert!(config.parent_of("nonexistent").is_none());

        let descendants = config.descendants_of("auth");
        assert_eq!(descendants.len(), 2);
        assert_eq!(descendants[0].name, "auth-tests");
        assert_eq!(descendants[1].name, "auth-ui");

        let descendants = config.descendants_of("auth-tests");
        assert_eq!(descendants.len(), 1);
        assert_eq!(descendants[0].name, "auth-ui");

        let descendants = config.descendants_of("auth-ui");
        assert_eq!(descendants.len(), 0);
    }

    #[test]
    fn test_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        atomic_write(&path, "hello = 'world'\n").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello = 'world'\n");
    }

    #[test]
    fn test_propagation_state_serialization() {
        let state = PropagationState {
            operation: Operation::Rebase,
            stack: "auth".to_string(),
            started_at: "2026-03-05T14:30:00Z".to_string(),
            original_branch: "feature/auth".to_string(),
            original_refs: vec![OriginalRef {
                branch: "feature/auth-tests".to_string(),
                commit: "aaa1111".to_string(),
            }],
            completed: vec![],
            remaining: vec!["feature/auth-tests".to_string()],
            current: Some("feature/auth-tests".to_string()),
        };

        let serialized = toml::to_string_pretty(&state).unwrap();
        let deserialized: PropagationState = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.operation, Operation::Rebase);
        assert_eq!(deserialized.stack, "auth");
        assert_eq!(deserialized.original_refs.len(), 1);
    }

    #[test]
    fn test_empty_stack_navigation() {
        let config = StackConfig {
            name: "empty".to_string(),
            base_branch: "dev".to_string(),
            branches: vec![],
        };

        assert!(config.root_branch().is_none());
        assert!(config.leaf_branch().is_none());
        assert!(config.parent_of("anything").is_none());
        assert_eq!(config.branch_index("anything"), None);
        assert!(config.descendants_of("anything").is_empty());
    }

    #[test]
    fn test_single_branch_stack() {
        let config = StackConfig {
            name: "solo".to_string(),
            base_branch: "main".to_string(),
            branches: vec![BranchEntry {
                name: "solo-branch".to_string(),
            }],
        };

        assert_eq!(config.root_branch().unwrap().name, "solo-branch");
        assert_eq!(config.leaf_branch().unwrap().name, "solo-branch");
        assert_eq!(config.parent_of("solo-branch").unwrap(), "main");
        assert!(config.descendants_of("solo-branch").is_empty());
    }

    #[test]
    fn test_stack_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-stack.toml");

        let config = StackConfig {
            name: "roundtrip".to_string(),
            base_branch: "dev".to_string(),
            branches: vec![
                BranchEntry {
                    name: "feature/a".to_string(),
                },
                BranchEntry {
                    name: "feature/b".to_string(),
                },
                BranchEntry {
                    name: "feature/c".to_string(),
                },
            ],
        };

        save_stack(&path, &config).unwrap();
        let loaded = load_stack(&path).unwrap();

        assert_eq!(loaded.name, config.name);
        assert_eq!(loaded.base_branch, config.base_branch);
        assert_eq!(loaded.branches.len(), 3);
        assert_eq!(loaded.branches[0].name, "feature/a");
        assert_eq!(loaded.branches[2].name, "feature/c");
    }

    #[test]
    fn test_propagation_state_save_load_remove() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.toml");

        // Initially no state
        assert!(load_propagation_state(&path).unwrap().is_none());

        let state = PropagationState {
            operation: Operation::Sync,
            stack: "billing".to_string(),
            started_at: "2026-03-05T15:00:00Z".to_string(),
            original_branch: "billing-api".to_string(),
            original_refs: vec![
                OriginalRef {
                    branch: "billing-ui".to_string(),
                    commit: "abc1234".to_string(),
                },
                OriginalRef {
                    branch: "billing-tests".to_string(),
                    commit: "def5678".to_string(),
                },
            ],
            completed: vec!["billing-ui".to_string()],
            remaining: vec!["billing-tests".to_string()],
            current: Some("billing-tests".to_string()),
        };

        save_propagation_state(&path, &state).unwrap();
        let loaded = load_propagation_state(&path).unwrap().unwrap();
        assert_eq!(loaded.operation, Operation::Sync);
        assert_eq!(loaded.original_refs.len(), 2);
        assert_eq!(loaded.completed.len(), 1);
        assert_eq!(loaded.remaining.len(), 1);

        remove_propagation_state(&path).unwrap();
        assert!(load_propagation_state(&path).unwrap().is_none());
    }

    #[test]
    fn test_atomic_write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("file.toml");
        atomic_write(&path, "content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "content");
    }

    #[test]
    fn test_load_stack_with_invalid_branch_name_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(
            &path,
            r#"
name = "ok"
base_branch = "main"

[[branches]]
name = "--malicious"
"#,
        )
        .unwrap();

        // Should fail validation
        let result = load_stack(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_all_operation_types_serialize() {
        for op in [
            Operation::Rebase,
            Operation::Sync,
            Operation::Adopt,
            Operation::BranchRemove,
        ] {
            let state = PropagationState {
                operation: op.clone(),
                stack: "test".to_string(),
                started_at: "2026-01-01T00:00:00Z".to_string(),
                original_branch: "main".to_string(),
                original_refs: vec![],
                completed: vec![],
                remaining: vec![],
                current: None,
            };
            let serialized = toml::to_string_pretty(&state).unwrap();
            let deserialized: PropagationState = toml::from_str(&serialized).unwrap();
            assert_eq!(deserialized.operation, op);
        }
    }
}

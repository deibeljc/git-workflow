use anyhow::{bail, Result};

/// Validate a commit SHA is a valid hex string.
/// Used to prevent injection when SHAs come from user input (plan files, state files).
pub fn validate_sha(sha: &str) -> Result<()> {
    if sha.is_empty() {
        bail!("Commit SHA cannot be empty");
    }
    if !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("Invalid commit SHA '{sha}': must contain only hex characters");
    }
    Ok(())
}

/// Validate a stack name for filesystem safety and git compatibility.
/// Stack names are used to construct file paths (.git/gw/stacks/<name>.toml),
/// so they must not contain path traversal sequences or filesystem-unsafe characters.
pub fn validate_stack_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Stack name cannot be empty");
    }

    if name.starts_with('-') {
        bail!("Stack name cannot start with '-'");
    }

    if name.contains("..") {
        bail!("Stack name cannot contain '..'");
    }

    if name.contains('/') || name.contains('\\') {
        bail!("Stack name cannot contain path separators ('/' or '\\')");
    }

    if name.contains('\0') {
        bail!("Stack name cannot contain null bytes");
    }

    // Only allow alphanumeric, hyphens, underscores, and dots
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        bail!("Stack name can only contain alphanumeric characters, hyphens, underscores, and dots");
    }

    Ok(())
}

/// Validate a branch name for git safety.
/// Branch names are passed as arguments to git commands, so they must not
/// be interpretable as flags or contain git-unsafe characters.
pub fn validate_branch_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Branch name cannot be empty");
    }

    if name.starts_with('-') {
        bail!("Branch name cannot start with '-' (would be interpreted as a git flag)");
    }

    if name.contains("..") {
        bail!("Branch name cannot contain '..' (git ref traversal)");
    }

    if name.contains('\0') {
        bail!("Branch name cannot contain null bytes");
    }

    if name.ends_with(".lock") {
        bail!("Branch name cannot end with '.lock'");
    }

    if name.ends_with('/') {
        bail!("Branch name cannot end with '/'");
    }

    // Check for git-unsafe characters
    for c in ['~', '^', ':', '?', '*', '[', '\\'] {
        if name.contains(c) {
            bail!("Branch name cannot contain '{c}'");
        }
    }

    // Check for control characters
    if name.chars().any(|c| c.is_control()) {
        bail!("Branch name cannot contain control characters");
    }

    // Check for spaces
    if name.contains(' ') {
        bail!("Branch name cannot contain spaces");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_stack_names() {
        assert!(validate_stack_name("auth").is_ok());
        assert!(validate_stack_name("my-feature").is_ok());
        assert!(validate_stack_name("feature_123").is_ok());
        assert!(validate_stack_name("v1.0").is_ok());
    }

    #[test]
    fn invalid_stack_names() {
        assert!(validate_stack_name("").is_err());
        assert!(validate_stack_name("-bad").is_err());
        assert!(validate_stack_name("../../etc").is_err());
        assert!(validate_stack_name("path/traversal").is_err());
        assert!(validate_stack_name("back\\slash").is_err());
        assert!(validate_stack_name("has spaces").is_err());
    }

    #[test]
    fn valid_branch_names() {
        assert!(validate_branch_name("feature/auth").is_ok());
        assert!(validate_branch_name("fix-bug-123").is_ok());
        assert!(validate_branch_name("my_branch").is_ok());
    }

    #[test]
    fn invalid_branch_names() {
        assert!(validate_branch_name("").is_err());
        assert!(validate_branch_name("--force").is_err());
        assert!(validate_branch_name("-flag").is_err());
        assert!(validate_branch_name("bad..ref").is_err());
        assert!(validate_branch_name("has space").is_err());
        assert!(validate_branch_name("file.lock").is_err());
        assert!(validate_branch_name("trail/").is_err());
        assert!(validate_branch_name("tilde~1").is_err());
        assert!(validate_branch_name("caret^2").is_err());
        assert!(validate_branch_name("colon:ref").is_err());
        assert!(validate_branch_name("question?").is_err());
        assert!(validate_branch_name("star*").is_err());
        assert!(validate_branch_name("bracket[0]").is_err());
        assert!(validate_branch_name("back\\slash").is_err());
        assert!(validate_branch_name("null\0byte").is_err());
        assert!(validate_branch_name("control\x01char").is_err());
    }

    #[test]
    fn branch_names_with_slashes_are_valid() {
        // Slashes are valid in branch names (feature/auth)
        assert!(validate_branch_name("feature/auth").is_ok());
        assert!(validate_branch_name("fix/bug-123").is_ok());
        assert!(validate_branch_name("user/jon/experiment").is_ok());
    }

    #[test]
    fn stack_names_with_null_bytes_rejected() {
        assert!(validate_stack_name("bad\0name").is_err());
    }

    #[test]
    fn valid_shas() {
        assert!(validate_sha("abc123").is_ok());
        assert!(validate_sha("deadbeef").is_ok());
        assert!(validate_sha("ABC123").is_ok());
        assert!(validate_sha("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2").is_ok());
    }

    #[test]
    fn invalid_shas() {
        assert!(validate_sha("").is_err());
        assert!(validate_sha("--exec=bad").is_err());
        assert!(validate_sha("not-hex!").is_err());
        assert!(validate_sha("abc 123").is_err());
        assert!(validate_sha("abc\n123").is_err());
    }

    #[test]
    fn stack_names_special_chars_rejected() {
        assert!(validate_stack_name("has spaces").is_err());
        assert!(validate_stack_name("semi;colon").is_err());
        assert!(validate_stack_name("pipe|char").is_err());
        assert!(validate_stack_name("ampersand&").is_err());
    }
}

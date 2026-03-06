use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::ui;

pub fn run() -> Result<()> {
    // Find the gw binary itself (the MCP server is a subcommand)
    let gw_path = find_gw()?;

    // Find or create .claude/settings.json in the repo root
    let git_root = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to find git root")?;

    let root = if git_root.status.success() {
        PathBuf::from(String::from_utf8_lossy(&git_root.stdout).trim())
    } else {
        // Not in a git repo, use home directory for global config
        PathBuf::from(std::env::var("HOME").context("HOME not set")?)
    };

    let settings_dir = root.join(".claude");
    let settings_path = settings_dir.join("settings.json");

    std::fs::create_dir_all(&settings_dir)
        .with_context(|| format!("failed to create {}", settings_dir.display()))?;

    // Load existing settings or start fresh
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)
            .with_context(|| format!("failed to read {}", settings_path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", settings_path.display()))?
    } else {
        serde_json::json!({})
    };

    // Set up mcpServers.gw pointing to `gw mcp-server`
    let mcp_servers = settings
        .as_object_mut()
        .context("settings.json is not an object")?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));

    let servers = mcp_servers
        .as_object_mut()
        .context("mcpServers is not an object")?;

    servers.insert(
        "gw".to_string(),
        serde_json::json!({
            "command": gw_path.to_string_lossy(),
            "args": ["mcp-server"],
        }),
    );

    // Write back
    let content = serde_json::to_string_pretty(&settings)
        .context("failed to serialize settings")?;
    std::fs::write(&settings_path, format!("{content}\n"))
        .with_context(|| format!("failed to write {}", settings_path.display()))?;

    ui::success(&format!("MCP server configured at {}", settings_path.display()));
    ui::info(&format!("Command: {} mcp-server", gw_path.display()));
    ui::info("Restart Claude Code to pick up the new MCP server.");

    Ok(())
}

fn find_gw() -> Result<PathBuf> {
    // Check if gw is on PATH (it should be since we're running it)
    if let Ok(output) = std::process::Command::new("which").arg("gw").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    // Fall back to current executable path
    if let Ok(exe) = std::env::current_exe() {
        return Ok(exe);
    }

    Ok(PathBuf::from("gw"))
}

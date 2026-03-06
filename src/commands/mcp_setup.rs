use anyhow::{bail, Context, Result};
use std::path::PathBuf;

use crate::ui;

pub fn run() -> Result<()> {
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        bail!("mcp-setup requires an interactive terminal.");
    }

    let gw_path = find_gw()?;

    // Ask the user where to install
    println!("Where should the MCP server be configured?\n");
    println!("  1) This project (.mcp.json in repo root)");
    println!("  2) Globally (~/.mcp.json for all projects)");
    println!();

    let global = loop {
        eprint!("Choice [1/2]: ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        match input.trim() {
            "1" => break false,
            "2" => break true,
            _ => eprintln!("Please enter 1 or 2."),
        }
    };

    let mcp_path = if global {
        PathBuf::from(std::env::var("HOME").context("HOME not set")?).join(".mcp.json")
    } else {
        let git_root = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .context("failed to find git root")?;

        let root = if git_root.status.success() {
            PathBuf::from(String::from_utf8_lossy(&git_root.stdout).trim())
        } else {
            std::env::current_dir().context("failed to get current directory")?
        };

        root.join(".mcp.json")
    };

    // Load existing .mcp.json or start fresh
    let mut config: serde_json::Value = if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path)
            .with_context(|| format!("failed to read {}", mcp_path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", mcp_path.display()))?
    } else {
        serde_json::json!({})
    };

    // Set up mcpServers.gw
    let mcp_servers = config
        .as_object_mut()
        .context(".mcp.json is not an object")?
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
    let content = serde_json::to_string_pretty(&config)
        .context("failed to serialize config")?;
    std::fs::write(&mcp_path, format!("{content}\n"))
        .with_context(|| format!("failed to write {}", mcp_path.display()))?;

    let scope = if global { "globally" } else { "for this project" };
    ui::success(&format!("MCP server configured {scope} at {}", mcp_path.display()));
    ui::info("Restart Claude Code to pick up the new MCP server.");

    Ok(())
}

fn find_gw() -> Result<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        return Ok(exe);
    }

    if let Ok(output) = std::process::Command::new("which").arg("gw").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    Ok(PathBuf::from("gw"))
}

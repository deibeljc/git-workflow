use std::collections::HashMap;
use std::process::Command;

/// PR information from gh CLI.
pub struct PrInfo {
    pub number: u64,
    pub state: String,
}

/// Fetch PR status for the given branches via parallel gh CLI calls.
/// Each branch gets its own `gh pr list --head <branch>` query, spawned
/// concurrently so the total latency is ~one network round-trip.
/// Returns a map of branch_name -> PrInfo.
/// Returns empty map if gh is not available or fails.
pub fn batch_pr_status(branches: &[&str]) -> HashMap<String, PrInfo> {
    let mut result = HashMap::new();
    if branches.is_empty() {
        return result;
    }

    // Spawn all queries in parallel
    let children: Vec<(&str, Option<std::process::Child>)> = branches
        .iter()
        .map(|&branch| {
            let child = Command::new("gh")
                .args([
                    "pr", "list", "--head", branch, "--state", "all", "--json",
                    "number,state", "--limit", "1",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .ok();
            (branch, child)
        })
        .collect();

    // Collect results
    for (branch, child) in children {
        let Some(child) = child else { continue };
        let Ok(output) = child.wait_with_output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Parse the first (and only) result from the JSON array
        for chunk in stdout.split('{') {
            if let (Some(number), Some(state)) = (
                extract_json_number(chunk, "number"),
                extract_json_string(chunk, "state"),
            ) {
                result.insert(branch.to_string(), PrInfo { number, state });
                break;
            }
        }
    }

    result
}

/// Check if a specific branch has a merged PR via the batched results.
pub fn is_branch_merged(pr_map: &HashMap<String, PrInfo>, branch: &str) -> bool {
    pr_map
        .get(branch)
        .is_some_and(|info| info.state == "MERGED")
}

/// Format PR status for display.
pub fn format_pr_status(info: &PrInfo) -> String {
    match info.state.as_str() {
        "OPEN" => format!("PR #{} open", info.number),
        "CLOSED" => format!("PR #{} closed", info.number),
        "MERGED" => format!("PR #{} merged", info.number),
        _ => format!("PR #{}", info.number),
    }
}

fn extract_json_string(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let start = text.find(&pattern)? + pattern.len();
    let end = text[start..].find('"')? + start;
    Some(text[start..end].to_string())
}

fn extract_json_number(text: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{key}\":");
    let start = text.find(&pattern)? + pattern.len();
    let num_str: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num_str.parse().ok()
}

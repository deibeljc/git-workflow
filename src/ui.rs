use std::io::{self, BufRead, Write};

/// Ask a yes/no confirmation question. Returns true for yes.
/// In non-interactive mode (stdin is not a TTY), returns the default value.
pub fn confirm(prompt: &str, default: bool) -> bool {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };

    // Check if stdin is a TTY
    if !atty_check() {
        return default;
    }

    print!("{prompt} {suffix} ");
    io::stdout().flush().ok();

    let mut input = String::new();
    if io::stdin().lock().read_line(&mut input).is_err() {
        return default;
    }

    let input = input.trim().to_lowercase();
    match input.as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        "" => default,
        _ => default,
    }
}

/// Check if stdin is interactive.
fn atty_check() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// Print a success message.
pub fn success(msg: &str) {
    use colored::Colorize;
    println!("{}", msg.green());
}

/// Print a warning message to stderr.
pub fn warn(msg: &str) {
    use colored::Colorize;
    eprintln!("{}", msg.yellow());
}

/// Print an error message to stderr.
pub fn error(msg: &str) {
    use colored::Colorize;
    eprintln!("{}", msg.red());
}

/// Prompt the user for text input with an optional default value.
/// In non-interactive mode (stdin is not a TTY), returns the default if provided,
/// otherwise returns an error.
pub fn prompt(message: &str, default: Option<&str>) -> io::Result<String> {
    if !atty_check() {
        return match default {
            Some(d) => Ok(d.to_string()),
            None => Err(io::Error::new(
                io::ErrorKind::Other,
                "non-interactive mode and no default provided",
            )),
        };
    }

    match default {
        Some(d) => print!("{message} [{d}]: "),
        None => print!("{message}: "),
    }
    io::stdout().flush().ok();

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;

    let input = input.trim();
    if input.is_empty() {
        match default {
            Some(d) => Ok(d.to_string()),
            None => Ok(String::new()),
        }
    } else {
        Ok(input.to_string())
    }
}

/// Output content, using a pager if stdout is a TTY and content overflows the terminal.
///
/// Pager selection: $GW_PAGER > $PAGER > less -R
/// When `focus_line` is provided and the pager is less, opens at that line number.
pub fn output_with_pager(content: &str, no_pager: bool, focus_line: Option<usize>) {
    use std::io::IsTerminal;

    if no_pager || !std::io::stdout().is_terminal() {
        print!("{content}");
        return;
    }

    // Use stderr terminal for size detection (stdout may be piped to pager)
    let term = console::Term::stderr();
    let (term_height, _) = term.size();
    let line_count = content.lines().count();

    if line_count <= term_height as usize {
        print!("{content}");
        return;
    }

    // Cap the focus line so we don't scroll past the end of content.
    // Place the target line ~1/3 down the screen for context above.
    let adjusted_line = focus_line.map(|line| {
        let offset = (term_height as usize) / 3;
        let start = line.saturating_sub(offset);
        let max_start = line_count.saturating_sub(term_height as usize) + 1;
        start.clamp(1, max_start)
    });

    pipe_to_pager(content, adjusted_line);
}

fn pipe_to_pager(content: &str, focus_line: Option<usize>) {
    use std::process::{Command, Stdio};

    let pager_env = std::env::var("GW_PAGER")
        .or_else(|_| std::env::var("PAGER"))
        .ok();

    let (cmd, args) = match &pager_env {
        Some(pager) => {
            let parts: Vec<&str> = pager.split_whitespace().collect();
            let cmd = parts[0].to_string();
            let mut args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
            if is_less(&cmd) {
                if !args.iter().any(|a| a.contains('R')) {
                    args.push("-R".to_string());
                }
                if let Some(line) = focus_line {
                    args.push(format!("+{line}"));
                }
            }
            (cmd, args)
        }
        None => {
            let mut args = vec!["-R".to_string()];
            if let Some(line) = focus_line {
                args.push(format!("+{line}"));
            }
            ("less".to_string(), args)
        }
    };

    let child = Command::new(&cmd)
        .args(&args)
        .stdin(Stdio::piped())
        .spawn();

    match child {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(content.as_bytes());
            }
            child.stdin.take();
            let _ = child.wait();
        }
        Err(_) => {
            print!("{content}");
        }
    }
}

fn is_less(cmd: &str) -> bool {
    std::path::Path::new(cmd)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("less"))
        .unwrap_or(false)
}

/// Print an info message.
pub fn info(msg: &str) {
    println!("{msg}");
}

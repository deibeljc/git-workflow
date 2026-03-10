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

/// Print an info message.
pub fn info(msg: &str) {
    println!("{msg}");
}

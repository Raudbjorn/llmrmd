use std::io::{BufRead, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

pub struct PreflightResult {
    pub cli_path: PathBuf,
    pub cli_version: String,
    pub auth_verified: bool,
}

/// Run preflight checks: binary detection, version, auth verification.
/// All I/O uses raw stderr — must run before tracing init.
pub fn preflight() -> Result<PreflightResult, String> {
    let cli_path = detect_or_install_binary()?;
    let cli_version = check_version(&cli_path);
    let auth_verified = verify_auth(&cli_path);

    Ok(PreflightResult {
        cli_path,
        cli_version,
        auth_verified,
    })
}

// -- Binary detection & installation ------------------------------------------

fn detect_or_install_binary() -> Result<PathBuf, String> {
    if let Ok(path) = which::which("claude") {
        return Ok(path);
    }

    eprintln!("claude binary not found in PATH.");

    let pm = detect_package_manager();
    match pm {
        Some((name, bin)) => {
            let cmd = format!("{bin} install -g @anthropic-ai/claude-code");
            if prompt_yn(&format!("Install with `{cmd}`?")) {
                run_install(&bin)?;
                which::which("claude").map_err(|_| {
                    "claude binary still not found after installation. Check your PATH.".to_string()
                })
            } else {
                Err(format!(
                    "claude binary required. Install manually: {name} install -g @anthropic-ai/claude-code"
                ))
            }
        }
        None => Err(
            "claude binary not found and no supported package manager (npm/pnpm/bun) detected.\n\
             Install manually: npm install -g @anthropic-ai/claude-code"
                .to_string(),
        ),
    }
}

/// Returns (display_name, binary_path) for the first available JS package manager.
fn detect_package_manager() -> Option<(&'static str, String)> {
    for name in ["pnpm", "bun", "npm"] {
        if let Ok(path) = which::which(name) {
            return Some((name, path.to_string_lossy().to_string()));
        }
    }
    None
}

fn run_install(pm_bin: &str) -> Result<(), String> {
    eprintln!("Installing @anthropic-ai/claude-code...");
    let status = Command::new(pm_bin)
        .args(["install", "-g", "@anthropic-ai/claude-code"])
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| format!("failed to run installer: {e}"))?;

    if status.success() {
        eprintln!("Installation complete.");
        Ok(())
    } else {
        Err(format!(
            "Installation failed (exit code: {})",
            status.code().unwrap_or(-1)
        ))
    }
}

// -- Version check ------------------------------------------------------------

fn check_version(cli_path: &PathBuf) -> String {
    let output = Command::new(cli_path)
        .arg("--version")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();

    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => "unknown".to_string(),
    }
}

// -- Auth verification --------------------------------------------------------

const AUTH_KEYWORDS: &[&str] = &[
    "login",
    "authenticate",
    "unauthorized",
    "session",
    "token",
    "sign in",
    "not logged in",
    "auth",
];

fn verify_auth(cli_path: &PathBuf) -> bool {
    match run_auth_probe(cli_path) {
        AuthProbeResult::Authenticated => true,
        AuthProbeResult::NotAuthenticated(stderr) => {
            eprintln!("Claude CLI may not be authenticated.");
            if !stderr.is_empty() {
                eprintln!("  {stderr}");
            }
            if prompt_yn("Run `claude` login flow now?") {
                run_login(cli_path);
                matches!(run_auth_probe(cli_path), AuthProbeResult::Authenticated)
            } else {
                eprintln!("Warning: skipping authentication — errors may surface per-request.");
                false
            }
        }
        AuthProbeResult::Unknown => {
            eprintln!("Warning: could not determine auth status — continuing anyway.");
            false
        }
    }
}

enum AuthProbeResult {
    Authenticated,
    NotAuthenticated(String),
    Unknown,
}

fn run_auth_probe(cli_path: &PathBuf) -> AuthProbeResult {
    let child = Command::new(cli_path)
        .args(["-p", "hi", "--output-format", "json"])
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(_) => return AuthProbeResult::Unknown,
    };

    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stderr = child
                    .stderr
                    .take()
                    .map(|s| {
                        let mut buf = String::new();
                        std::io::BufReader::new(s)
                            .read_to_string(&mut buf)
                            .ok();
                        buf
                    })
                    .unwrap_or_default();

                if status.success() {
                    return AuthProbeResult::Authenticated;
                }

                let stderr_lower = stderr.to_lowercase();
                if AUTH_KEYWORDS.iter().any(|kw| stderr_lower.contains(kw)) {
                    let first_line = stderr.lines().next().unwrap_or("").trim().to_string();
                    return AuthProbeResult::NotAuthenticated(first_line);
                }

                return AuthProbeResult::Unknown;
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return AuthProbeResult::Unknown;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return AuthProbeResult::Unknown,
        }
    }
}

fn run_login(cli_path: &PathBuf) {
    eprintln!("Starting Claude login flow...");
    let status = Command::new(cli_path)
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    match status {
        Ok(s) if s.success() => eprintln!("Login flow completed."),
        Ok(s) => eprintln!(
            "Login flow exited with code {}.",
            s.code().unwrap_or(-1)
        ),
        Err(e) => eprintln!("Failed to start login flow: {e}"),
    }
}

// -- Helpers ------------------------------------------------------------------

/// Prompt `[Y/n]` on stderr, read one line from stdin.
/// Defaults to `true` on empty input, `false` on EOF.
fn prompt_yn(msg: &str) -> bool {
    eprint!("{msg} [Y/n] ");
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) => false, // EOF (non-TTY / piped)
        Ok(_) => {
            let trimmed = line.trim().to_lowercase();
            trimmed.is_empty() || trimmed.starts_with('y')
        }
        Err(_) => false,
    }
}

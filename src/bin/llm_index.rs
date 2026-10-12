//! Backward-compatible `llm-index` binary.
//! Delegates to `llmrmd index`.

use std::process::ExitCode;

fn main() -> ExitCode {
    // Rewrite args: llm-index [args...] → llmrmd index [args...]
    let args: Vec<String> = std::env::args().collect();
    let mut new_args = vec!["llmrmd".to_string(), "index".to_string()];
    new_args.extend(args.into_iter().skip(1));

    // Re-parse with clap by setting env
    std::env::set_var("CARGO_BIN_NAME", "llmrmd");
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(&new_args[1..])
        .status();

    match result {
        Ok(status) => {
            if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

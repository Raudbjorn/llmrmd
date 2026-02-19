//! CLI entry point for llmermaid.
//!
//! Subcommands:
//! - `index`  — scan repo, produce file index + diagram manifest
//! - `plan`   — assemble planning context for a proposed change
//! - (default) — launch interactive TUI

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing::{error, info};

#[derive(Parser)]
#[command(
    name = "llmermaid",
    about = "Indexer + Planner + TUI for LLM-assisted monorepo development",
    version,
    after_help = "Run with no subcommand to launch the interactive TUI."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Root directory of the monorepo (default: current directory).
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,

    /// Enable verbose (debug) logging.
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Scan a monorepo and produce TOON file index + diagram manifest.
    Index {
        /// Print output to stdout instead of writing files.
        #[arg(long)]
        dry_run: bool,
    },

    /// Assemble minimal planning context for a proposed change.
    Plan {
        /// Natural-language description of the proposed change.
        change: Option<String>,

        /// Soft token budget for the planning context.
        #[arg(long, default_value = "4000")]
        budget: usize,

        /// Comma-separated list of scopes (overrides auto-detection).
        #[arg(long)]
        scopes: Option<String>,

        /// Print the planning prompt to stdout instead of writing to file.
        #[arg(long)]
        stdout: bool,

        /// Call the Anthropic API to generate a structured plan.
        /// Requires ANTHROPIC_API_KEY environment variable.
        #[arg(long)]
        call_api: bool,

        /// Model to use for API calls (default: claude-opus-4-6).
        #[arg(long)]
        model: Option<String>,

        /// List saved plans instead of creating a new one.
        #[arg(long)]
        list: bool,

        /// Approve a saved plan by its slug.
        #[arg(long)]
        approve: Option<String>,

        /// Show details of a saved plan by its slug.
        #[arg(long)]
        show: Option<String>,
    },
}

fn setup_logging(verbose: bool) {
    use tracing_subscriber::EnvFilter;

    let filter = if verbose {
        EnvFilter::new("llmermaid=debug")
    } else {
        EnvFilter::new("llmermaid=info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    setup_logging(cli.verbose);

    let root = cli.root.canonicalize().unwrap_or(cli.root.clone());

    if !root.is_dir() {
        error!(path = %root.display(), "Root path does not exist or is not a directory");
        return ExitCode::FAILURE;
    }

    let result = match cli.command {
        Some(Command::Index { dry_run }) => run_index(&root, dry_run),
        Some(Command::Plan {
            change,
            budget,
            scopes,
            stdout,
            call_api,
            model,
            list,
            approve,
            show,
        }) => {
            // Plan management subcommands (no change description needed)
            if list {
                return match run_list_plans(&root) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        error!(error = %e, "Fatal error");
                        ExitCode::FAILURE
                    }
                };
            }
            if let Some(slug) = approve {
                return match run_approve_plan(&root, &slug) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        error!(error = %e, "Fatal error");
                        ExitCode::FAILURE
                    }
                };
            }
            if let Some(slug) = show {
                return match run_show_plan(&root, &slug) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        error!(error = %e, "Fatal error");
                        ExitCode::FAILURE
                    }
                };
            }

            run_plan(&root, change, budget, scopes, stdout, call_api, model)
        }
        None => run_tui(&root),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(error = %e, "Fatal error");
            ExitCode::FAILURE
        }
    }
}

fn run_index(root: &std::path::Path, dry_run: bool) -> llmermaid::error::Result<()> {
    info!(root = %root.display(), "Starting index");

    let result = llmermaid::indexer::scan_repo(root)?;

    if result.files.is_empty() {
        info!("No files found — nothing to index.");
        return Ok(());
    }

    llmermaid::indexer::write_index(&result, dry_run)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_plan(
    root: &std::path::Path,
    change: Option<String>,
    budget: usize,
    scopes: Option<String>,
    to_stdout: bool,
    call_api: bool,
    model: Option<String>,
) -> llmermaid::error::Result<()> {
    // Get change description from arg or stdin
    let change_desc = match change {
        Some(c) if !c.is_empty() => c,
        _ => {
            use std::io::{IsTerminal, Read};
            if std::io::stdin().is_terminal() {
                error!(
                    "No change description provided.\n\
                     Usage: llmermaid plan 'Add a locations table'\n\
                     Or:    echo 'Add auth middleware' | llmermaid plan"
                );
                return Err(llmermaid::error::Error::EmptyDescription);
            }
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| llmermaid::error::Error::Terminal(e.to_string()))?;
            buf.trim().to_string()
        }
    };

    if change_desc.is_empty() {
        return Err(llmermaid::error::Error::EmptyDescription);
    }

    let manifest = llmermaid::planner::load_manifest(root)?;
    let files = llmermaid::planner::load_file_index(root)?;

    let explicit_scopes: Option<Vec<String>> = scopes.map(|s| {
        s.split(',')
            .map(|scope| scope.trim().to_string())
            .filter(|scope| !scope.is_empty())
            .collect()
    });

    let ctx = llmermaid::planner::select_context_with_scopes(
        root,
        &change_desc,
        &manifest,
        &files,
        budget,
        explicit_scopes.as_deref(),
    );

    if call_api {
        return run_plan_with_api(root, &ctx, &files, &manifest, model.as_deref());
    }

    if to_stdout {
        let prompt = llmermaid::planner::render_planning_prompt(&ctx);
        print!("{prompt}");
        info!(
            scopes = ctx.relevant_scopes.len(),
            diagrams = ctx.selected_diagrams.len(),
            tokens = ctx.total_tokens_est,
            "Planning context generated"
        );
    } else {
        llmermaid::planner::write_planning_context(root, &ctx)?;
    }

    Ok(())
}

#[cfg(feature = "api")]
fn run_plan_with_api(
    root: &std::path::Path,
    ctx: &llmermaid::planner::types::PlannerContext,
    files: &[llmermaid::indexer::types::FileRecord],
    manifest: &[llmermaid::planner::types::ManifestEntry],
    model: Option<&str>,
) -> llmermaid::error::Result<()> {
    use llmermaid::api;
    use tracing::warn;

    let client = api::client::AnthropicClient::from_env()?;
    let request = api::build_plan_request(ctx, model);

    info!("Calling Anthropic API...");
    let response = client.call(&request)?;

    let plan_response = api::parse_plan_response(&response)?;

    // Validate each plan variant
    let mut all_warnings = Vec::new();
    for (i, plan) in plan_response.plans.iter().enumerate() {
        let validation = api::validate::validate_plan(plan, files, manifest);
        if !validation.is_valid() {
            error!(
                variant = i + 1,
                errors = ?validation.errors,
                "Plan variant has validation errors"
            );
            return Err(llmermaid::error::Error::PlanValidation(
                validation.errors.join("; "),
            ));
        }
        for w in &validation.warnings {
            warn!(variant = i + 1, warning = w, "Plan validation warning");
            all_warnings.push(format!("Variant {}: {w}", i + 1));
        }
    }

    // Save the plan
    let slug = llmermaid::planner::plans::save_plan(
        root,
        ctx,
        &plan_response,
        &request.model,
        Some(&response.usage),
        all_warnings,
    )?;

    // Print brief
    let persisted = llmermaid::planner::plans::load_plan(root, &slug)?;
    println!("{}", llmermaid::planner::plans::render_brief(&persisted));

    Ok(())
}

#[cfg(not(feature = "api"))]
fn run_plan_with_api(
    _root: &std::path::Path,
    _ctx: &llmermaid::planner::types::PlannerContext,
    _files: &[llmermaid::indexer::types::FileRecord],
    _manifest: &[llmermaid::planner::types::ManifestEntry],
    _model: Option<&str>,
) -> llmermaid::error::Result<()> {
    Err(llmermaid::error::Error::Config(
        "API support not compiled. Rebuild with: cargo build --features api".to_string(),
    ))
}

fn run_list_plans(root: &std::path::Path) -> llmermaid::error::Result<()> {
    let plans = llmermaid::planner::plans::list_plans(root)?;
    if plans.is_empty() {
        println!("No plans found. Run `llmermaid plan --call-api 'your change'` to create one.");
        return Ok(());
    }

    println!("Plans ({}):\n", plans.len());
    for plan in &plans {
        println!("{}", llmermaid::planner::plans::render_brief(plan));
        println!();
    }
    Ok(())
}

fn run_approve_plan(root: &std::path::Path, slug: &str) -> llmermaid::error::Result<()> {
    llmermaid::planner::plans::approve_plan(root, slug)?;
    println!("Plan '{slug}' approved.");
    Ok(())
}

fn run_show_plan(root: &std::path::Path, slug: &str) -> llmermaid::error::Result<()> {
    let plan = llmermaid::planner::plans::load_plan(root, slug)?;
    println!("{}", llmermaid::planner::plans::render_brief(&plan));
    Ok(())
}

fn run_tui(root: &std::path::Path) -> llmermaid::error::Result<()> {
    llmermaid::tui::run(root)
}

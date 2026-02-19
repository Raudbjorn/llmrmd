"""
CLI entry points for llmermaid.

Commands:
  llm-index  — Scan a monorepo and produce TOON file index + diagram manifest
  llm-plan   — Assemble minimal planning context for a proposed change
"""

from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path

log = logging.getLogger("llm_architect")


def _setup_logging(verbose: bool = False) -> None:
    """Configure structured logging."""
    level = logging.DEBUG if verbose else logging.INFO
    logging.basicConfig(
        level=level,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S",
    )


# ---------------------------------------------------------------------------
# llm-index
# ---------------------------------------------------------------------------


def main_index() -> None:
    """Entry point for the `llm-index` command."""
    parser = argparse.ArgumentParser(
        prog="llm-index",
        description="Scan a monorepo and produce a TOON file index + diagram manifest.",
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=Path.cwd(),
        help="Root directory of the monorepo (default: current directory)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print output to stdout instead of writing files",
    )
    parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Enable debug logging",
    )

    args = parser.parse_args()
    _setup_logging(args.verbose)

    root = args.root.resolve()
    if not root.is_dir():
        log.error("Root path does not exist or is not a directory: %s", root)
        sys.exit(1)

    from llm_architect.indexer import scan_repo, write_index

    log.info("Starting index of %s", root)
    result = scan_repo(root)

    if not result.files:
        log.warning("No files found in %s", root)
        sys.exit(0)

    exit_code = write_index(result, dry_run=args.dry_run)
    if exit_code != 0:
        log.error("Indexing failed with exit code %d", exit_code)

    sys.exit(exit_code)


# ---------------------------------------------------------------------------
# llm-plan
# ---------------------------------------------------------------------------


def main_plan() -> None:
    """Entry point for the `llm-plan` command."""
    parser = argparse.ArgumentParser(
        prog="llm-plan",
        description="Assemble minimal planning context for a proposed change.",
    )
    parser.add_argument(
        "change",
        nargs="?",
        help="Natural-language description of the proposed change",
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=Path.cwd(),
        help="Root directory of the monorepo (default: current directory)",
    )
    parser.add_argument(
        "--budget",
        type=int,
        default=4000,
        help="Soft token budget for the planning context (default: 4000)",
    )
    parser.add_argument(
        "--scopes",
        type=str,
        default="",
        help="Comma-separated list of scopes to include (overrides auto-detection)",
    )
    parser.add_argument(
        "--stdout",
        action="store_true",
        help="Print the planning prompt to stdout instead of writing to file",
    )
    parser.add_argument(
        "-v", "--verbose",
        action="store_true",
        help="Enable debug logging",
    )

    args = parser.parse_args()
    _setup_logging(args.verbose)

    root = args.root.resolve()
    if not root.is_dir():
        log.error("Root path does not exist or is not a directory: %s", root)
        sys.exit(1)

    # Get the change description
    if args.change:
        change_desc = args.change
    else:
        # Try to read from stdin
        if not sys.stdin.isatty():
            change_desc = sys.stdin.read().strip()
        else:
            log.error(
                "No change description provided. Usage:\n"
                "  llm-plan 'Add a locations table with geospatial columns'\n"
                "  echo 'Add auth middleware' | llm-plan"
            )
            sys.exit(1)

    if not change_desc:
        log.error("Empty change description.")
        sys.exit(1)

    from llm_architect.planner import (
        load_file_index,
        load_manifest,
        render_planning_prompt,
        select_context,
        write_planning_context,
    )

    # Load artifacts from the indexer
    manifest = load_manifest(root)
    file_index = load_file_index(root)

    if not file_index:
        log.error(
            "No file index found. Run `llm-index` first to generate the index artifacts."
        )
        sys.exit(1)

    # Assemble context
    context = select_context(
        root,
        change_desc,
        manifest,
        file_index,
        token_budget=args.budget,
    )

    # Override scopes if specified
    if args.scopes:
        explicit_scopes = [s.strip() for s in args.scopes.split(",") if s.strip()]
        if explicit_scopes:
            log.info("Overriding auto-detected scopes with: %s", explicit_scopes)
            context.relevant_scopes = explicit_scopes
            # Re-select with explicit scopes — simplified: just re-run
            context = select_context(
                root,
                change_desc,
                manifest,
                file_index,
                token_budget=args.budget,
            )

    if args.stdout:
        prompt = render_planning_prompt(context)
        sys.stdout.write(prompt)
        log.info(
            "Planning context: %d scopes, %d diagrams, ~%d tokens",
            len(context.relevant_scopes),
            len(context.selected_diagrams),
            context.total_tokens_est,
        )
    else:
        exit_code = write_planning_context(root, context)
        if exit_code != 0:
            log.error("Planning context generation failed.")
        sys.exit(exit_code)


# ---------------------------------------------------------------------------
# Standalone execution
# ---------------------------------------------------------------------------


def main() -> None:
    """Dispatch based on how we're invoked."""
    prog = Path(sys.argv[0]).stem if sys.argv else ""

    if "plan" in prog:
        main_plan()
    elif "index" in prog:
        main_index()
    else:
        # Default: show help for both
        print("llmermaid — Indexer + Planner for LLM-assisted development\n")
        print("Commands:")
        print("  llm-index   Scan repo and produce TOON file index + diagram manifest")
        print("  llm-plan    Assemble planning context for a proposed change")
        print()
        print("Run `llm-index --help` or `llm-plan --help` for details.")
        sys.exit(0)


if __name__ == "__main__":
    main()

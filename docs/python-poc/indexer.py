"""
Indexer: Scans a monorepo and produces structured artifacts for LLM context.

Outputs:
  .claude/file-index.toon    — compact file listing (TOON tabular format)
  .claude/file-index.json    — same data as JSON for programmatic use
  .claude/manifest.toon      — diagram inventory with scope, type, token estimate
  .claude/manifest.json      — same as JSON
  .claude/diagrams/extracted/ — mermaid diagrams extracted from source files
"""

from __future__ import annotations

import json
import logging
import os
import re
import sys
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from llm_architect.config import (
    DOMAIN_CONTAINER_DIRS,
    DOMAIN_LEAF_DIRS,
    EXTENSION_TYPE_MAP,
    FILENAME_TYPE_MAP,
    MAX_MERMAID_SCAN_BYTES,
    MERMAID_FILE_EXTENSIONS,
    MERMAID_SOURCE_EXTENSIONS,
    SKIP_DIRS,
    SKIP_FILE_EXACT,
    SKIP_FILE_SUFFIXES,
    output_dir,
)

log = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------

# Rough estimate: 1 token ≈ 4 chars for English text / code
CHARS_PER_TOKEN_ESTIMATE = 4

# Regex for ```mermaid ... ``` fenced blocks
_MERMAID_FENCE_RE = re.compile(
    r"```mermaid\s*\n(.*?)\n\s*```",
    re.DOTALL | re.IGNORECASE,
)


@dataclass(frozen=True, slots=True)
class FileRecord:
    """A single file in the index."""

    path: str
    file_type: str
    domain: str
    claude_md: str  # path to nearest CLAUDE.md, or empty


@dataclass(frozen=True, slots=True)
class DiagramRecord:
    """A mermaid diagram found in the repo."""

    source: str        # file path (+ #diag-N suffix if multiple)
    scope: str         # domain / directory scope
    diagram_type: str  # inferred mermaid diagram type (flowchart, erDiagram, etc.)
    tokens_est: int    # estimated token count
    content: str       # the minified mermaid source


@dataclass
class IndexResult:
    """Complete result of an indexing run."""

    root: Path
    generated_at: str
    files: list[FileRecord] = field(default_factory=list)
    diagrams: list[DiagramRecord] = field(default_factory=list)
    claude_md_paths: list[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _should_skip_file(rel_path: Path) -> bool:
    """Check if a file should be excluded from the index."""
    # Check directory components
    for part in rel_path.parts[:-1]:
        if part in SKIP_DIRS:
            return True

    name = rel_path.name

    # Exact name match
    if name in SKIP_FILE_EXACT:
        return True

    # Suffix match
    suffix = rel_path.suffix.lower()
    if suffix in SKIP_FILE_SUFFIXES:
        return True

    # Hidden files (but not .env variants)
    if name.startswith(".") and not name.startswith(".env"):
        return True

    return False


def _infer_type(path: Path) -> str:
    """Infer the semantic type of a file from its name/extension."""
    # Check filename overrides first
    if path.name in FILENAME_TYPE_MAP:
        return FILENAME_TYPE_MAP[path.name]

    return EXTENSION_TYPE_MAP.get(path.suffix.lower(), "file")


def _infer_domain(rel_path: Path) -> str:
    """
    Infer the domain (logical grouping) from the file's position in the repo.

    Container dirs (apps/, packages/) use the next level as domain:
      apps/web/src/lib/foo.ts → domain = "web"
      packages/shared/index.ts → domain = "shared"

    Leaf dirs (supabase/, infra/) ARE the domain:
      supabase/migrations/init.sql → domain = "supabase"
      infra/terraform/main.tf → domain = "infra"

    Root-level files → domain = "root"
    """
    parts = rel_path.parts
    if len(parts) <= 1:
        return "root"

    top = parts[0]

    # Container dirs: apps/web → "web", packages/shared → "shared"
    if top in DOMAIN_CONTAINER_DIRS and len(parts) > 2:
        return parts[1]

    # Leaf dirs: supabase/anything → "supabase"
    if top in DOMAIN_LEAF_DIRS:
        return top

    return top


def _find_claude_md_dirs(root: Path) -> set[Path]:
    """Walk the repo and find all directories containing a CLAUDE.md."""
    dirs: set[Path] = set()
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        if "CLAUDE.md" in filenames:
            dirs.add(Path(dirpath))
    return dirs


def _nearest_claude_md(file_path: Path, claude_md_dirs: set[Path], root: Path) -> str:
    """Find the nearest CLAUDE.md for a given file, walking up to root."""
    current = file_path.parent
    while True:
        if current in claude_md_dirs:
            try:
                return str((current / "CLAUDE.md").relative_to(root))
            except ValueError:
                return ""
        if current == root or current == current.parent:
            # Check root itself
            if root in claude_md_dirs:
                return "CLAUDE.md"
            return ""
        current = current.parent


def _estimate_tokens(text: str) -> int:
    """Rough token estimate. O(1) — just character count / 4."""
    return max(1, len(text) // CHARS_PER_TOKEN_ESTIMATE)


def _infer_mermaid_type(content: str) -> str:
    """Infer the mermaid diagram type from the first meaningful line."""
    for line in content.splitlines():
        stripped = line.strip().lower()
        if not stripped or stripped.startswith("%%"):
            continue

        if stripped.startswith("graph ") or stripped.startswith("graph\t"):
            return "flowchart"
        if stripped.startswith("flowchart"):
            return "flowchart"
        if stripped.startswith("sequencediagram"):
            return "sequence"
        if stripped.startswith("classdiagram"):
            return "classDiagram"
        if stripped.startswith("erdiagram"):
            return "erDiagram"
        if stripped.startswith("statediagram"):
            return "stateDiagram"
        if stripped.startswith("gantt"):
            return "gantt"
        if stripped.startswith("pie"):
            return "pie"
        if stripped.startswith("gitgraph"):
            return "gitGraph"
        if stripped.startswith("c4"):
            return "c4"
        if stripped.startswith("mindmap"):
            return "mindmap"
        if stripped.startswith("timeline"):
            return "timeline"
        if stripped.startswith("journey"):
            return "journey"
        if stripped.startswith("sankey"):
            return "sankey"
        if stripped.startswith("xychart"):
            return "xyChart"
        if stripped.startswith("block"):
            return "block"
        if stripped.startswith("architecture"):
            return "architecture"

        # Default: assume flowchart
        return "unknown"

    return "unknown"


def _minify_mermaid(diagram: str) -> str:
    """Strip comments and excess whitespace from a mermaid diagram."""
    lines: list[str] = []
    for line in diagram.splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("%%"):
            lines.append(stripped)
    return "\n".join(lines)


def _extract_mermaid_from_file(abs_path: Path) -> list[str]:
    """Extract mermaid diagram content from a file."""
    suffix = abs_path.suffix.lower()
    if suffix not in MERMAID_SOURCE_EXTENSIONS:
        return []

    # Size guard
    try:
        size = abs_path.stat().st_size
        if size > MAX_MERMAID_SCAN_BYTES:
            log.debug("Skipping mermaid scan for large file: %s (%d bytes)", abs_path, size)
            return []
    except OSError:
        return []

    try:
        content = abs_path.read_text(encoding="utf-8", errors="ignore")
    except OSError as exc:
        log.warning("Could not read file for mermaid extraction: %s — %s", abs_path, exc)
        return []

    # Dedicated mermaid files: entire content is the diagram
    if suffix in MERMAID_FILE_EXTENSIONS:
        return [content] if content.strip() else []

    # Otherwise, look for fenced blocks
    return _MERMAID_FENCE_RE.findall(content)


# ---------------------------------------------------------------------------
# TOON Encoding (spec-compliant tabular format)
# ---------------------------------------------------------------------------
# We use the `toons` library for encoding when available, with a lightweight
# fallback for the specific tabular arrays we produce (uniform dicts with
# primitive string values). This avoids a hard dependency on a Rust extension.


def _try_import_toons() -> bool:
    """Check if the toons library is available."""
    try:
        import toons  # noqa: F401

        return True
    except ImportError:
        return False


def _escape_toon_value(val: str, delimiter: str = ",") -> str:
    """
    Quote a TOON value if it contains the delimiter, colon, quotes,
    control chars, or has leading/trailing whitespace.

    Per TOON spec §7: strings are unquoted unless they'd be ambiguous.
    """
    if not val:
        return '""'

    needs_quotes = (
        delimiter in val
        or ":" in val
        or '"' in val
        or "\n" in val
        or "\r" in val
        or "\t" in val
        or "[" in val
        or "]" in val
        or "{" in val
        or "}" in val
        or val.startswith(" ")
        or val.endswith(" ")
        or val.startswith("-")
        or val == "true"
        or val == "false"
        or val == "null"
    )

    if needs_quotes:
        escaped = (
            val.replace("\\", "\\\\")
            .replace('"', '\\"')
            .replace("\n", "\\n")
            .replace("\r", "\\r")
            .replace("\t", "\\t")
        )
        return f'"{escaped}"'

    return val


def _render_toon_tabular(
    array_name: str,
    fields: list[str],
    rows: list[dict[str, str]],
    *,
    comment: str = "",
) -> str:
    """
    Render a TOON tabular array.

    Produces output like:
        # <comment>
        files[123]{path,type,domain,claude_md}:
        apps/web/src/index.ts,module,web,apps/web/CLAUDE.md
        ...
    """
    lines: list[str] = []
    if comment:
        lines.append(f"# {comment}")

    field_header = ",".join(fields)
    lines.append(f"{array_name}[{len(rows)}]{{{field_header}}}:")

    for row in rows:
        values = [_escape_toon_value(str(row.get(f, ""))) for f in fields]
        lines.append(",".join(values))

    return "\n".join(lines)


def _render_full_toon(result: IndexResult) -> str:
    """Render the complete file-index.toon output."""
    sections: list[str] = []

    # Header comment
    sections.append(f"# REPO FILE INDEX — generated {result.generated_at}")
    sections.append(f"# Root: {result.root.name}")
    sections.append(f"# Files: {len(result.files)}  Diagrams: {len(result.diagrams)}")
    sections.append("")

    # CLAUDE.md locations
    if result.claude_md_paths:
        sections.append("# CLAUDE.md locations (progressive disclosure chain)")
        claude_rows = [{"path": p} for p in sorted(result.claude_md_paths)]
        sections.append(_render_toon_tabular("claude_docs", ["path"], claude_rows))
        sections.append("")

    # File index
    file_rows = [
        {"path": f.path, "type": f.file_type, "domain": f.domain, "claude_md": f.claude_md}
        for f in result.files
    ]
    sections.append(
        _render_toon_tabular(
            "files", ["path", "type", "domain", "claude_md"], file_rows,
            comment="FILE INDEX",
        )
    )

    return "\n".join(sections) + "\n"


def _render_manifest_toon(result: IndexResult) -> str:
    """Render the manifest.toon — a lightweight diagram inventory."""
    sections: list[str] = []

    sections.append(f"# DIAGRAM MANIFEST — generated {result.generated_at}")
    sections.append(f"# Use this to select which diagrams to load for planning.")
    sections.append("")

    if not result.diagrams:
        sections.append("# No diagrams found.")
        return "\n".join(sections) + "\n"

    manifest_rows = [
        {
            "source": d.source,
            "scope": d.scope,
            "type": d.diagram_type,
            "tokens": str(d.tokens_est),
        }
        for d in result.diagrams
    ]
    sections.append(
        _render_toon_tabular(
            "diagrams", ["source", "scope", "type", "tokens"], manifest_rows,
            comment="AVAILABLE DIAGRAMS (load selectively based on task scope)",
        )
    )

    return "\n".join(sections) + "\n"


# ---------------------------------------------------------------------------
# Main indexer logic
# ---------------------------------------------------------------------------


def scan_repo(root: Path) -> IndexResult:
    """
    Walk the repository and collect file records and mermaid diagrams.

    Returns an IndexResult with all collected data.
    Errors during individual file processing are logged and skipped.
    """
    root = root.resolve()
    if not root.is_dir():
        log.error("Repo root does not exist or is not a directory: %s", root)
        return IndexResult(
            root=root,
            generated_at=datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        )

    log.info("Scanning repo: %s", root)

    # Phase 1: find all CLAUDE.md locations
    claude_md_dirs = _find_claude_md_dirs(root)
    claude_md_paths: list[str] = []
    for d in sorted(claude_md_dirs):
        try:
            claude_md_paths.append(str((d / "CLAUDE.md").relative_to(root)))
        except ValueError:
            pass

    log.info("Found %d CLAUDE.md files", len(claude_md_paths))

    # Phase 2: walk and collect
    files: list[FileRecord] = []
    diagrams: list[DiagramRecord] = []

    for dirpath_str, dirnames, filenames in os.walk(root):
        # Prune skipped directories in-place
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)

        for filename in sorted(filenames):
            abs_path = Path(dirpath_str) / filename
            try:
                rel_path = abs_path.relative_to(root)
            except ValueError:
                continue

            if _should_skip_file(rel_path):
                continue

            posix_path = rel_path.as_posix()

            # File record
            files.append(
                FileRecord(
                    path=posix_path,
                    file_type=_infer_type(abs_path),
                    domain=_infer_domain(rel_path),
                    claude_md=_nearest_claude_md(abs_path, claude_md_dirs, root),
                )
            )

            # Mermaid extraction
            raw_diagrams = _extract_mermaid_from_file(abs_path)
            for i, raw_content in enumerate(raw_diagrams):
                minified = _minify_mermaid(raw_content)
                if not minified:
                    continue

                source = (
                    f"{posix_path}#diag-{i + 1}"
                    if len(raw_diagrams) > 1
                    else posix_path
                )
                diagrams.append(
                    DiagramRecord(
                        source=source,
                        scope=_infer_domain(rel_path),
                        diagram_type=_infer_mermaid_type(minified),
                        tokens_est=_estimate_tokens(minified),
                        content=minified,
                    )
                )

    log.info(
        "Collected %d files, extracted %d mermaid diagrams",
        len(files),
        len(diagrams),
    )

    return IndexResult(
        root=root,
        generated_at=datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        files=files,
        diagrams=diagrams,
        claude_md_paths=claude_md_paths,
    )


def write_index(result: IndexResult, *, dry_run: bool = False) -> int:
    """
    Write all index artifacts to the .claude/ directory.

    Returns 0 on success, 1 on error.
    """
    if dry_run:
        toon_str = _render_full_toon(result)
        manifest_str = _render_manifest_toon(result)
        sys.stdout.write(toon_str)
        sys.stdout.write("\n")
        sys.stdout.write(manifest_str)
        log.info("Dry run complete. %d files, %d diagrams.", len(result.files), len(result.diagrams))
        return 0

    out = output_dir(result.root)

    try:
        out.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        log.error("Failed to create output directory %s: %s", out, exc)
        return 1

    # --- file-index.toon ---
    toon_str = _render_full_toon(result)
    toon_path = out / "file-index.toon"
    try:
        toon_path.write_text(toon_str, encoding="utf-8")
        log.info("Wrote %s (%d bytes)", toon_path, toon_path.stat().st_size)
    except OSError as exc:
        log.error("Failed to write %s: %s", toon_path, exc)
        return 1

    # --- file-index.json ---
    json_data = {
        "generated_at": result.generated_at,
        "root": result.root.name,
        "claude_md_paths": result.claude_md_paths,
        "files": [asdict(f) for f in result.files],
    }
    json_path = out / "file-index.json"
    try:
        json_path.write_text(json.dumps(json_data, indent=2), encoding="utf-8")
        log.info("Wrote %s (%d bytes)", json_path, json_path.stat().st_size)
    except OSError as exc:
        log.error("Failed to write %s: %s", json_path, exc)
        return 1

    # --- manifest.toon ---
    manifest_str = _render_manifest_toon(result)
    manifest_path = out / "manifest.toon"
    try:
        manifest_path.write_text(manifest_str, encoding="utf-8")
        log.info("Wrote %s (%d bytes)", manifest_path, manifest_path.stat().st_size)
    except OSError as exc:
        log.error("Failed to write %s: %s", manifest_path, exc)
        return 1

    # --- manifest.json ---
    manifest_json = {
        "generated_at": result.generated_at,
        "diagrams": [
            {
                "source": d.source,
                "scope": d.scope,
                "type": d.diagram_type,
                "tokens_est": d.tokens_est,
            }
            for d in result.diagrams
        ],
    }
    manifest_json_path = out / "manifest.json"
    try:
        manifest_json_path.write_text(json.dumps(manifest_json, indent=2), encoding="utf-8")
        log.info("Wrote %s (%d bytes)", manifest_json_path, manifest_json_path.stat().st_size)
    except OSError as exc:
        log.error("Failed to write %s: %s", manifest_json_path, exc)
        return 1

    # --- diagrams/extracted/*.mmd ---
    if result.diagrams:
        diag_dir = out / "diagrams" / "extracted"
        try:
            diag_dir.mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            log.error("Failed to create diagrams directory %s: %s", diag_dir, exc)
            return 1

        for diag in result.diagrams:
            # Sanitize filename: replace / with __ and remove # suffixes
            safe_name = diag.source.replace("/", "__").replace("#", "_")
            if not safe_name.endswith(".mmd"):
                safe_name += ".mmd"

            diag_path = diag_dir / safe_name
            try:
                diag_path.write_text(diag.content, encoding="utf-8")
            except OSError as exc:
                log.warning("Failed to write diagram %s: %s", diag_path, exc)

        log.info("Wrote %d diagrams to %s", len(result.diagrams), diag_dir)

    log.info(
        "Done. %d files indexed, %d diagrams extracted → %s/",
        len(result.files),
        len(result.diagrams),
        out,
    )
    return 0

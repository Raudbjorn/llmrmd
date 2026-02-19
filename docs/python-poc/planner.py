"""
Planner: Assembles minimal context from indexer artifacts for LLM planning.

The planner reads the manifest and file index, then based on a natural-language
change description, selects the relevant diagrams and CLAUDE.md files to produce
a compact "planning context" that fits in a small context window.

It outputs:
  .claude/planner-context.md  — ready-to-paste planning prompt
"""

from __future__ import annotations

import json
import logging
import re
from dataclasses import dataclass, field
from pathlib import Path
from string import Template

from llm_architect.config import output_dir

log = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------


@dataclass
class ManifestEntry:
    """A diagram entry from the manifest."""
    source: str
    scope: str
    diagram_type: str
    tokens_est: int


@dataclass
class PlannerContext:
    """All the context assembled for a planning session."""
    change_description: str
    relevant_scopes: list[str]
    system_diagram: str  # always included — the root-level diagram if any
    selected_diagrams: list[tuple[ManifestEntry, str]]  # (entry, content)
    relevant_claude_mds: list[tuple[str, str]]  # (path, content)
    file_index_excerpt: str  # filtered TOON for relevant domains
    total_tokens_est: int = 0


# ---------------------------------------------------------------------------
# Manifest loading
# ---------------------------------------------------------------------------


def load_manifest(root: Path) -> list[ManifestEntry]:
    """Load the diagram manifest from .claude/manifest.json."""
    manifest_path = output_dir(root) / "manifest.json"
    if not manifest_path.exists():
        log.warning("No manifest found at %s. Run the indexer first.", manifest_path)
        return []

    try:
        data = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        log.error(
            "Failed to load manifest from %s. Error: %s",
            manifest_path, exc,
        )
        return []

    entries: list[ManifestEntry] = []
    for d in data.get("diagrams", []):
        entries.append(ManifestEntry(
            source=d.get("source", ""),
            scope=d.get("scope", ""),
            diagram_type=d.get("type", "unknown"),
            tokens_est=int(d.get("tokens_est", 0)),
        ))

    log.info("Loaded manifest with %d diagram entries", len(entries))
    return entries


def load_file_index(root: Path) -> dict:
    """Load the file index from .claude/file-index.json."""
    index_path = output_dir(root) / "file-index.json"
    if not index_path.exists():
        log.warning("No file index found at %s. Run the indexer first.", index_path)
        return {}

    try:
        return json.loads(index_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        log.error("Failed to load file index from %s: %s", index_path, exc)
        return {}


# ---------------------------------------------------------------------------
# Scope inference
# ---------------------------------------------------------------------------


def infer_scopes(
    change_description: str,
    manifest: list[ManifestEntry],
    file_index: dict,
) -> list[str]:
    """
    Infer which scopes (domains) are relevant to the change description.

    Uses keyword matching against:
      - domain names from the file index
      - diagram sources from the manifest
      - common architectural terms
    """
    description_lower = change_description.lower()

    # Collect all known domains
    all_domains: set[str] = set()
    for f in file_index.get("files", []):
        domain = f.get("domain", "")
        if domain:
            all_domains.add(domain)
    for entry in manifest:
        if entry.scope:
            all_domains.add(entry.scope)

    # Simple keyword matching: if the domain name appears in the description
    matched: list[str] = []
    for domain in sorted(all_domains):
        if domain.lower() in description_lower:
            matched.append(domain)

    # Also check for common architectural keywords that map to domains
    keyword_map: dict[str, list[str]] = {
        "database": ["supabase", "migrations", "db", "prisma"],
        "schema": ["supabase", "migrations", "db", "prisma"],
        "migration": ["supabase", "migrations"],
        "table": ["supabase", "migrations"],
        "column": ["supabase", "migrations"],
        "rls": ["supabase"],
        "postgis": ["supabase"],
        "sql": ["supabase"],
        "api": ["admin", "web", "api"],
        "endpoint": ["admin", "web", "api"],
        "route": ["admin", "web"],
        "crud": ["admin"],
        "dashboard": ["admin"],
        "page": ["admin", "web"],
        "auth": ["auth", "shared"],
        "frontend": ["web"],
        "admin": ["admin"],
        "ui": ["web", "admin"],
        "shared": ["shared"],
        "type": ["shared", "types"],
        "types": ["shared"],
        "interface": ["shared"],
        "validator": ["shared"],
        "deploy": ["infra", "deploy"],
        "infra": ["infra"],
        "ci": ["infra", "scripts"],
        "docker": ["infra"],
        "terraform": ["infra"],
    }

    for keyword, domains in keyword_map.items():
        if keyword in description_lower:
            for d in domains:
                if d in all_domains and d not in matched:
                    matched.append(d)

    if not matched:
        log.info("No specific scopes matched. Will use root-level context only.")
        matched = ["root"]

    log.info("Inferred scopes for planning: %s", matched)
    return matched


# ---------------------------------------------------------------------------
# Context selection
# ---------------------------------------------------------------------------


def _read_diagram_content(root: Path, source: str) -> str:
    """
    Read diagram content. Tries:
      1. The extracted .mmd file in .claude/diagrams/extracted/
      2. The original source file
    """
    # Try extracted location
    safe_name = source.replace("/", "__").replace("#", "_")
    if not safe_name.endswith(".mmd"):
        safe_name += ".mmd"

    extracted = output_dir(root) / "diagrams" / "extracted" / safe_name
    if extracted.exists():
        try:
            return extracted.read_text(encoding="utf-8")
        except OSError:
            pass

    # Try original source (strip #diag-N suffix)
    original_path = source.split("#")[0]
    abs_path = root / original_path
    if abs_path.exists():
        try:
            return abs_path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            pass

    log.warning("Could not read diagram content for: %s", source)
    return ""


def _read_claude_md(root: Path, claude_md_path: str) -> str:
    """Read a CLAUDE.md file's content."""
    abs_path = root / claude_md_path
    if abs_path.exists():
        try:
            return abs_path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            pass
    return ""


def _filter_file_index_toon(file_index: dict, scopes: list[str]) -> str:
    """
    Produce a filtered TOON excerpt showing only files in the relevant scopes.

    This gives the planner a focused view of *where* files live without
    loading the entire index.
    """
    files = file_index.get("files", [])
    relevant = [f for f in files if f.get("domain") in scopes or "root" in scopes]

    if not relevant:
        return "# No files in selected scopes."

    # Render as lightweight TOON-style table
    lines: list[str] = [f"# Filtered file index ({len(relevant)} files in scopes: {', '.join(scopes)})"]
    lines.append(f"files[{len(relevant)}]{{path,type,domain}}:")

    for f in relevant:
        path = f.get("path", "")
        file_type = f.get("file_type", f.get("type", "file"))
        domain = f.get("domain", "")
        lines.append(f"{path},{file_type},{domain}")

    return "\n".join(lines)


TOKEN_BUDGET_DEFAULT = 4000  # soft limit for total planning context


def select_context(
    root: Path,
    change_description: str,
    manifest: list[ManifestEntry],
    file_index: dict,
    *,
    token_budget: int = TOKEN_BUDGET_DEFAULT,
) -> PlannerContext:
    """
    Select the minimal context needed for planning a change.

    Strategy:
      1. Always include root-level system diagram (if exists)
      2. Infer relevant scopes from the change description
      3. Select diagrams that match those scopes (within token budget)
      4. Include CLAUDE.md files for matched scopes
      5. Produce a filtered file index excerpt
    """
    scopes = infer_scopes(change_description, manifest, file_index)

    # 1. System diagram (root scope, usually tiny)
    system_diagram = ""
    for entry in manifest:
        if entry.scope == "root":
            content = _read_diagram_content(root, entry.source)
            if content:
                system_diagram = content
                break

    tokens_used = len(system_diagram) // 4  # rough estimate

    # 2. Select scope-relevant diagrams
    selected: list[tuple[ManifestEntry, str]] = []
    for entry in sorted(manifest, key=lambda e: e.tokens_est):
        if entry.scope in scopes and entry.scope != "root":
            if tokens_used + entry.tokens_est > token_budget:
                log.info(
                    "Skipping diagram %s (%d tokens) — would exceed budget (%d/%d)",
                    entry.source, entry.tokens_est, tokens_used, token_budget,
                )
                continue

            content = _read_diagram_content(root, entry.source)
            if content:
                selected.append((entry, content))
                tokens_used += entry.tokens_est

    # 3. Relevant CLAUDE.md files
    claude_md_paths = file_index.get("claude_md_paths", [])
    relevant_claude_mds: list[tuple[str, str]] = []
    for cpath in claude_md_paths:
        # Match if the CLAUDE.md is in a directory that matches a scope
        path_parts = Path(cpath).parts
        for scope in scopes:
            if scope in path_parts or scope == "root":
                content = _read_claude_md(root, cpath)
                if content:
                    relevant_claude_mds.append((cpath, content))
                    tokens_used += len(content) // 4
                break

    # 4. Filtered file index
    file_excerpt = _filter_file_index_toon(file_index, scopes)
    tokens_used += len(file_excerpt) // 4

    context = PlannerContext(
        change_description=change_description,
        relevant_scopes=scopes,
        system_diagram=system_diagram,
        selected_diagrams=selected,
        relevant_claude_mds=relevant_claude_mds,
        file_index_excerpt=file_excerpt,
        total_tokens_est=tokens_used,
    )

    log.info(
        "Planning context assembled: %d scopes, %d diagrams, %d CLAUDE.md files, ~%d tokens",
        len(scopes), len(selected), len(relevant_claude_mds), tokens_used,
    )

    return context


# ---------------------------------------------------------------------------
# Prompt rendering
# ---------------------------------------------------------------------------


PLANNER_SYSTEM_PROMPT = """\
You are a software architect and implementation planner. Your job is to analyze
a proposed change against the current system architecture and produce a precise
implementation plan.

## Your inputs

1. **System diagram** — high-level view of how components relate
2. **Scope diagrams** — detailed views of the affected components
3. **CLAUDE.md files** — conventions, patterns, and pointers for each scope
4. **File index** — what files exist in the relevant areas

## Your outputs

Produce ALL of the following:

### 1. Impact Analysis
Which components/scopes are affected? What are the cross-cutting concerns?
Are there any dependencies between the changes?

### 2. Updated Diagrams
For each affected diagram, produce an **updated** version showing the target
state AFTER the change. Use valid Mermaid syntax. Wrap each in a fenced block:
```mermaid
<updated diagram>
```

### 3. Implementation Plan
A numbered list of concrete steps. Each step MUST specify:
- **File**: exact path (create new or modify existing)
- **Action**: create | modify | delete | move
- **Description**: what changes in 1-2 sentences
- **Depends on**: which earlier steps must complete first (if any)

Order steps by dependency — independent steps first, dependent steps later.

### 4. Context Pointers (for handoff)
List the minimal set of files/docs that the *implementing* LLM needs in its
context window. Format as a checklist:
- [ ] `path/to/file` — reason it's needed

### 5. Risk & Questions
Flag anything ambiguous, potentially breaking, or that needs human decision.

## Rules
- Follow conventions from CLAUDE.md files exactly
- Reference existing patterns (don't invent new ones when existing ones work)
- Keep the plan minimal — smallest change that achieves the goal
- If a step requires a new file, suggest placement based on existing structure
- Prefer modifying existing files over creating new ones where sensible
"""


def render_planning_prompt(context: PlannerContext) -> str:
    """
    Render the complete planning prompt ready for an LLM.

    Structure:
      1. System prompt (instructions)
      2. System diagram (always)
      3. Scope diagrams (selected)
      4. CLAUDE.md content (relevant)
      5. File index excerpt
      6. The change description (user's task)
    """
    sections: list[str] = []

    # System prompt
    sections.append(PLANNER_SYSTEM_PROMPT)
    sections.append("---\n")

    # System diagram
    if context.system_diagram:
        sections.append("## System Architecture (current state)\n")
        sections.append(f"```mermaid\n{context.system_diagram}\n```\n")

    # Scope diagrams
    if context.selected_diagrams:
        sections.append("## Component Diagrams (current state)\n")
        for entry, content in context.selected_diagrams:
            sections.append(
                f"### {entry.source} ({entry.diagram_type}, ~{entry.tokens_est} tokens)\n"
            )
            sections.append(f"```mermaid\n{content}\n```\n")

    # CLAUDE.md files
    if context.relevant_claude_mds:
        sections.append("## Project Conventions\n")
        for path, content in context.relevant_claude_mds:
            sections.append(f"### {path}\n")
            sections.append(f"{content}\n")

    # File index excerpt
    sections.append("## Relevant Files\n")
    sections.append(f"```toon\n{context.file_index_excerpt}\n```\n")

    # The actual task
    sections.append("---\n")
    sections.append("## Change Request\n")
    sections.append(f"{context.change_description}\n")
    sections.append("\n---\n")
    sections.append(
        f"*Planning context: {len(context.relevant_scopes)} scopes, "
        f"{len(context.selected_diagrams)} diagrams, "
        f"~{context.total_tokens_est} tokens*\n"
    )

    return "\n".join(sections)


HANDOFF_TEMPLATE = """\
# Implementation Handoff

## Task
$change_description

## Implementation Plan
$plan_content

## Target Architecture
$updated_diagrams

## Context Files to Load
$context_pointers

## Conventions
$conventions_summary

---
*This handoff was generated by llmermaid planner. The context above is
the minimal set needed for implementation — do NOT load additional files
unless the plan explicitly requires them.*
"""


def render_handoff(
    change_description: str,
    plan_content: str,
    updated_diagrams: str,
    context_pointers: str,
    conventions_summary: str,
) -> str:
    """
    Render the implementation handoff document.

    This is the compact output that gets injected into a FRESH context window
    for the implementing LLM — no bloated history, just focused instructions.
    """
    template = Template(HANDOFF_TEMPLATE)
    return template.safe_substitute(
        change_description=change_description,
        plan_content=plan_content,
        updated_diagrams=updated_diagrams,
        context_pointers=context_pointers,
        conventions_summary=conventions_summary,
    )


def write_planning_context(root: Path, context: PlannerContext) -> int:
    """
    Write the planning context to .claude/planner-context.md.

    Returns 0 on success, 1 on error.
    """
    out = output_dir(root)
    try:
        out.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        log.error("Failed to create output directory %s: %s", out, exc)
        return 1

    prompt = render_planning_prompt(context)
    output_path = out / "planner-context.md"

    try:
        output_path.write_text(prompt, encoding="utf-8")
        log.info(
            "Wrote planning context → %s (%d bytes, ~%d tokens)",
            output_path,
            output_path.stat().st_size,
            context.total_tokens_est,
        )
    except OSError as exc:
        log.error("Failed to write planning context: %s", exc)
        return 1

    return 0

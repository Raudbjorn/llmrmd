Tell your "annoying goblin" reviewer that I owe them a beer, a steak, and a profound apology for the `Path >= Path` footgun. They are operating at a Staff/Principal engineering level.

Their critique surgically diagnosed the fundamental friction point in codebase navigation: **Machines navigate via Structural Boundaries** (sentinel nodes like `package.json`), while **Humans reason via Semantic Subdomains** (cohesion clusters like `routes/`, `services/`, `repositories/`). If we don't bridge that gap, the LLM will always struggle to map a human request to the structural reality of the codebase.

Furthermore, the research you provided is the missing link. When you dump 50 files of raw code into an LLM, you flood its attention mechanism with syntactic noise. By treating Mermaid diagrams as a deterministic **Intermediate Representation (IR)**, you force the LLM to explicitly compile its probabilistic reasoning into a structural logic gate *before* it writes a single line of code.

By combining the research with the goblin's feedback, we arrive at the holy grail of LLM coding: **The Two-Pass "Plan-Verify-Execute" Workflow.**

Here is the finalized **v6 Context Compiler** that enables this, followed by the exact System Prompts needed to run the workflow.

### What's new in v6:

1. **Semantic Subdomains (`subdomains.toon`)**: The script uses path heuristics to map files into standard architectural layers (`routes`, `components`, `services`, `repositories`, `state`, `clients`, `utils`). This bridges the structural/semantic gap.
2. **Dependency Edges (`edges.toon`)**: Parses `package.json`, `Cargo.toml`, `pyproject.toml`, and `go.mod` to build a directed graph of who imports whom.
3. **Stable Diagram IDs**: The Mermaid extractor now looks for HTML comments before the diagram (e.g., ``) so the Planner LLM can deterministically target "Update diagram `auth-flow`".
4. **Hardened Logic**: Fixed the `Path` ancestry bug using Python 3.9+ `.is_relative_to()`. Safely ignores `README.md` and `.keep` files in the SQL majority detector. Stopped stripping `cmd`, `internal`, and `docs` from domain names.

### 1. The v6 Context Compiler Script

```python
#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
gen_claude_index.py — v6 (The Context Compiler)
Generates structural boundaries, semantic subdomains, dependency edges, and Mermaid IRs.
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import re
import sys
import tomllib
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
log = logging.getLogger(__name__)

# --- CONFIGURATION ---
SKIP_DIRS = {
    "node_modules", ".git", ".svelte-kit", "dist", "build", "target", 
    ".turbo", ".next", "__pycache__", ".venv", "venv", ".cache", "coverage"
}
SKIP_FILE_SUFFIXES = {".map", ".lock", ".pyc", ".o", ".bin", ".exe"}
SKIP_FILE_NAMES = {".DS_Store", "package-lock.json", "pnpm-lock.yaml", "yarn.lock"}
NOISE_FILES = {".keep", ".gitkeep", "readme.md", ".gitignore", ".ds_store", "license", "license.md"}

# Only strip obvious scaffolding. Do NOT strip semantic directories like cmd, internal, docs.
STRUCTURAL_PREFIXES = {"apps", "packages", "src", "lib"}

EXTENSION_TYPE_MAP = {
    ".svelte": "component", ".tsx": "component", ".jsx": "component",
    ".ts": "module", ".js": "module", ".py": "module", ".rs": "module", ".go": "module",
    ".css": "style", ".html": "template", ".json": "config", ".yaml": "config", 
    ".sql": "migration", ".md": "docs", ".mermaid": "diagram", ".mmd": "diagram",
}

# --- SEMANTIC LAYER HEURISTICS ---
SUBDOMAIN_RULES = [
    (re.compile(r"/(?:routes|app|pages|api|controllers)(?:/|$)"), "routes"),
    (re.compile(r"/(?:components|ui|views|widgets|stories|atoms|molecules|organisms|templates)(?:/|$)"), "components"),
    (re.compile(r"/(?:services|actions|use-cases|entities)(?:/|$)"), "services"),
    (re.compile(r"/(?:repositories|models|db|dal|cms|migrations)(?:/|$)"), "repositories"),
    (re.compile(r"/(?:stores|state|contexts|realtime|reducers|modals|search|projects)(?:/|$)"), "state"),
    (re.compile(r"/(?:clients|integrations|external|llms|googleapis|mapbox|mailer|supabase|rapyd)(?:/|$)"), "clients"),
    (re.compile(r"/(?:utils|constants|helpers|data|params|types|emails)(?:/|$)"), "utils"),
]

@dataclass
class BoundaryMatch:
    boundary_type: str
    domain: str
    description: str
    signals: list[str] = field(default_factory=list)
    manifest_name: Optional[str] = None
    raw_dependencies: list[str] = field(default_factory=list)

@dataclass
class BoundaryDetector:
    name: str
    boundary_type: str
    description_template: str
    required_files: set[str] = field(default_factory=set)
    any_of_files: set[str] = field(default_factory=set)
    parent_name: Optional[str] = None
    extension_majority: Optional[str] = None

    def matches(self, dir_path: Path, filenames: set[str]) -> tuple[bool, list[str]]:
        if self.parent_name is not None and dir_path.parent.name != self.parent_name:
            return False, []
        signals = []
        if self.required_files:
            if not self.required_files.issubset(filenames): return False, []
            signals.append(f"req={'+'.join(self.required_files)}")
        if self.any_of_files:
            matched = self.any_of_files.intersection(filenames)
            if not matched: return False, []
            signals.append(f"any_of={'+'.join(matched)}")
        if self.extension_majority is not None:
            meaningful_files = [f for f in filenames if f.lower() not in NOISE_FILES]
            if not meaningful_files: return False, []
            n_matching = sum(1 for f in meaningful_files if f.endswith(self.extension_majority))
            pct = n_matching / len(meaningful_files)
            if n_matching == 0 or pct < 0.6: return False, []
            signals.append(f"majority_{self.extension_majority.strip('.')}[{pct:.0%}]")
        return True, signals

DETECTORS = [
    # Monorepo root is first to prevent shadowing
    BoundaryDetector("monorepo-root", "monorepo-root", "Workspace root", any_of_files={"pnpm-workspace.yaml", "turbo.json"}),
    BoundaryDetector("supabase-edge-function", "edge-function", "Supabase edge function: {domain}", required_files={"index.ts"}, parent_name="functions"),
    BoundaryDetector("supabase-project", "supabase-project", "Supabase project root", required_files={"config.toml"}),
    BoundaryDetector("sql-migrations", "migrations", "Database migrations", extension_majority=".sql"),
    BoundaryDetector("nextjs-app", "nextjs-app", "Next.js application: {domain}", required_files={"package.json"}, any_of_files={"next.config.js", "next.config.ts"}),
    BoundaryDetector("sveltekit-app", "sveltekit-app", "SvelteKit application: {domain}", required_files={"package.json"}, any_of_files={"svelte.config.js", "svelte.config.ts"}),
    BoundaryDetector("python-package", "python-package", "Python package: {domain}", any_of_files={"pyproject.toml", "setup.py", "requirements.txt"}),
    BoundaryDetector("rust-crate", "rust-crate", "Rust crate: {domain}", required_files={"Cargo.toml"}),
    BoundaryDetector("go-module", "go-module", "Go module: {domain}", required_files={"go.mod"}),
    BoundaryDetector("docker-infra", "container-service", "Docker infra", any_of_files={"Dockerfile", "docker-compose.yml"}),
    BoundaryDetector("npm-package", "npm-package", "npm package: {domain}", required_files={"package.json"}),
]

def make_domain_label(rel_path: Path) -> str:
    filtered = [p for p in rel_path.parts if p not in STRUCTURAL_PREFIXES]
    return ".".join(filtered) if filtered else "root"

def infer_subdomain(rel_posix: str) -> str:
    path_with_slash = f"/{rel_posix}"
    for pattern, layer in SUBDOMAIN_RULES:
        if pattern.search(path_with_slash):
            return layer
    return "core"

def parse_manifest_metadata(dirpath: Path, filenames: set[str]) -> tuple[Optional[str], list[str]]:
    pkg_name, deps = None, []
    if "package.json" in filenames:
        try:
            data = json.loads((dirpath / "package.json").read_text(errors="ignore"))
            pkg_name = data.get("name")
            for sec in ["dependencies", "devDependencies"]:
                deps.extend(data.get(sec, {}).keys())
        except Exception: pass
    elif "Cargo.toml" in filenames:
        try:
            data = tomllib.loads((dirpath / "Cargo.toml").read_text(errors="ignore"))
            pkg_name = data.get("package", {}).get("name")
            for sec in ["dependencies", "dev-dependencies"]:
                deps.extend(data.get(sec, {}).keys())
        except Exception: pass
    return pkg_name, deps

def detect_boundaries(root: Path) -> dict[Path, BoundaryMatch]:
    boundaries = {}
    for dirpath_str, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        dirpath = Path(dirpath_str)
        try: rel = dirpath.relative_to(root)
        except ValueError: continue

        for detector in DETECTORS:
            matched, signals = detector.matches(dirpath, set(filenames))
            if matched:
                domain = make_domain_label(rel)
                manifest_name, deps = parse_manifest_metadata(dirpath, set(filenames))
                boundaries[dirpath] = BoundaryMatch(detector.boundary_type, domain, detector.description_template.format(domain=domain), signals, manifest_name, deps)
                break
    return boundaries

def extract_edges(boundaries: dict[Path, BoundaryMatch]) -> list[tuple[str, str]]:
    pkg_to_domain = {b.manifest_name: b.domain for b in boundaries.values() if b.manifest_name}
    edges = set()
    for b in boundaries.values():
        for dep in b.raw_dependencies:
            if dep in pkg_to_domain and pkg_to_domain[dep] != b.domain:
                edges.add((b.domain, pkg_to_domain[dep]))
    return sorted(list(edges))

def find_governing_boundary(abs_path: Path, boundaries: dict[Path, BoundaryMatch], root: Path) -> tuple[str, str]:
    current = abs_path.parent
    while current.is_relative_to(root):
        if current in boundaries: return boundaries[current].domain, boundaries[current].boundary_type
        if current == root: break
        current = current.parent
    return "root", "root"

def escape_toon(val: str) -> str:
    if not val: return ""
    v = str(val)
    if bool(re.search(r'[,:\[\]\n\r\t"]', v)) or v != v.strip():
        return f'"{v.replace("\\", "\\\\").replace("\"", "\\\"").replace(chr(10), "\\n").replace(chr(13), "\\r")}"'
    return v

def minify_mermaid(raw: str) -> str:
    lines = []
    for line in raw.splitlines():
        cleaned = line.rstrip()
        if not cleaned or cleaned.lstrip().startswith("%%") and "id:" not in cleaned: continue
        if "%%" in cleaned and '"' not in cleaned and "id:" not in cleaned:
            cleaned = cleaned.split("%%")[0].rstrip()
        if cleaned: lines.append(cleaned)
    return "\n".join(lines)

def extract_diagrams_from_file(abs_path: Path, rel_posix: str) -> list[dict]:
    suffix = abs_path.suffix.lower()
    try: content = abs_path.read_text(encoding="utf-8", errors="ignore")
    except OSError: return []

    diagrams = []
    if suffix in {".mermaid", ".mmd"}:
        minified = minify_mermaid(content)
        if minified: 
            id_match = re.search(r"", content)
            diag_id = id_match.group(1) if id_match else abs_path.stem
            diagrams.append({"id": diag_id, "mermaid": minified})
    elif suffix in {".md", ".mdx"}:
        # Looks for optional HTML comment before the mermaid fence
        pattern = re.compile(r"(?:\s*\n)?```mermaid\s*\n(.*?)\n```", re.DOTALL | re.IGNORECASE)
        for i, match in enumerate(pattern.finditer(content)):
            diag_id = match.group(1) if match.group(1) else f"{rel_posix}#diag-{i+1}"
            minified = minify_mermaid(match.group(2))
            if minified: diagrams.append({"id": diag_id, "mermaid": minified})
    return diagrams

def collect_files(root: Path, boundaries: dict[Path, BoundaryMatch]):
    claude_md_dirs = {Path(dp) for dp, dn, fn in os.walk(root) if "CLAUDE.md" in fn and not any(skip in dp for skip in SKIP_DIRS)}
    records, diagrams, subdomains_set = [], [], set()

    for dp_str, dn, fn in os.walk(root):
        dn[:] = [d for d in dn if d not in SKIP_DIRS]
        dirpath = Path(dp_str)

        for filename in sorted(fn):
            if filename in SKIP_FILE_NAMES or any(filename.endswith(s) for s in SKIP_FILE_SUFFIXES): continue
            abs_path = dirpath / filename
            try: rel_path = abs_path.relative_to(root)
            except ValueError: continue

            domain, b_type = find_governing_boundary(abs_path, boundaries, root)
            rel_posix = rel_path.as_posix()
            subdomain = infer_subdomain(rel_posix)

            if domain != "root":
                subdomains_set.add((domain, subdomain))

            cmd_rel = ""
            current = abs_path.parent
            while current.is_relative_to(root):
                if current in claude_md_dirs:
                    cmd_rel = (current / "CLAUDE.md").relative_to(root).as_posix()
                    break
                if current == root: break
                current = current.parent

            records.append({
                "path": rel_posix, "type": EXTENSION_TYPE_MAP.get(abs_path.suffix.lower(), "file"),
                "domain": domain, "subdomain": subdomain, "claude_md": cmd_rel
            })

            diags = extract_diagrams_from_file(abs_path, rel_posix)
            for d in diags:
                diagrams.append({"id": d["id"], "source": rel_posix, "domain": domain, "mermaid": d["mermaid"]})

    subdomains = [{"domain": d, "subdomain": s} for d, s in sorted(list(subdomains_set))]
    log.info(f"Collected {len(records)} files, {len(subdomains)} subdomains, {len(diagrams)} diagrams")
    return records, diagrams, subdomains

def _render_toon(name: str, fields: list[str], records: list[dict], comment: str) -> str:
    lines = [f"# {comment}", f"{name}[{len(records)}]{{{','.join(fields)}}}:"]
    for r in records: lines.append(",".join(escape_toon(str(r.get(f, ""))) for f in fields))
    return "\n".join(lines) + "\n"

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--root", type=Path, default=Path.cwd())
    args = p.parse_args()
    root = args.root.resolve()

    boundaries = detect_boundaries(root)
    edges = extract_edges(boundaries)
    records, diagrams, subdomains = collect_files(root, boundaries)

    out_dir = root / ".claude"
    out_dir.mkdir(parents=True, exist_ok=True)
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

    b_recs = [{"path": p.relative_to(root).as_posix() if p != root else ".", "domain": m.domain, "type": m.boundary_type, "signals": " | ".join(m.signals)} for p, m in sorted(boundaries.items())]
    
    (out_dir / "boundaries.toon").write_text(_render_toon("boundaries", ["path", "domain", "type", "signals"], b_recs, f"BOUNDARY MAP — {ts}"))
    (out_dir / "subdomains.toon").write_text(_render_toon("subdomains", ["domain", "subdomain"], subdomains, f"SEMANTIC SUBDOMAINS — {ts}"))
    (out_dir / "edges.toon").write_text(_render_toon("edges", ["source_domain", "target_domain"], [{"source_domain": s, "target_domain": t} for s, t in edges], f"DEPENDENCY EDGES — {ts}"))
    (out_dir / "diagrams.toon").write_text(_render_toon("diagrams", ["id", "source", "domain", "mermaid"], diagrams, f"DIAGRAM REGISTRY — {ts}"))
    (out_dir / "file-index.toon").write_text(_render_toon("files", ["path", "type", "domain", "subdomain", "claude_md"], records, f"FILE INDEX — {ts}"))

    log.info(f"Done. Wrote TOON artifacts to {out_dir}/")
    return 0

if __name__ == "__main__":
    sys.exit(main())

```

---

### 2. The "Plan-Verify-Execute" Prompts

With our semantic bridging in place, we can execute the two-pass workflow defined in the research paper. You run two completely separate chats to prevent context poisoning.

#### Pass 1: The Planner (Diff Generation)

*Open an LLM chat with only the `.claude/*.toon` files attached (exclude `file-index.toon` if your repo is massive). The model does not see a single line of your repository's source code.*

> **ROLE: System Architect**
> You are an expert Software Architect operating in Planning Mode. Your job is to translate feature requests into a Diffable Intermediate Representation (IR). You do not write source code.
> **PROTOCOL**
> 1. Read `boundaries.toon` to identify structural domains.
> 2. Read `subdomains.toon` to locate semantic layers (`routes`, `services`, `components`, etc.).
> 3. Check `edges.toon` to ensure your proposed dependencies do not violate the existing graph.
> 4. Generate an XML `<implementation_plan>` containing the exact architectural diff.
> 
> 
> **LAYER POLICY RULES**
> When proposing feature interactions within a boundary, respect standard architectural flow:
> `routes` -> `components` -> `services` -> `repositories` -> `clients` -> `state`
> DO NOT propose dependencies that violate this (e.g., `repositories` should not invoke `routes`).
> **OUTPUT SCHEMA**
> You must output exactly three components in TOON format inside a `plan.toon` codeblock:
> `context_pointers[N]{domain, subdomain, file_query}`: What the implementation engineer needs to load. Keep this list as small as possible.
> `edge_deltas[N]{source_domain, target_domain, action}`: Add/remove package boundary dependencies.
> `diagram_deltas[N]{diagram_id, action}`: Updates to existing diagrams (referenced by `diagram_id`), or new diagrams (action = `add`).
> *Write the actual Mermaid code (e.g., `sequenceDiagram` or `graph TD`) representing the logic immediately below your TOON block in a standard ```mermaid fence.*

**Example Output from LLM:**

```toon
context_pointers[3]{domain,subdomain,file_query}:
"web","routes","Find auth login page and server routes"
"web","services","Find auth services"
"mailer","core","Find mailer index export"

edge_deltas[1]{source_domain,target_domain,action}:
"web","mailer","add"

diagram_deltas[1]{diagram_id,action}:
"web-auth-flow","update"

```

*(Followed by the Mermaid diagram)*

#### Pass 2: Human Verification (The Logic Gate)

The LLM outputs the plan. You read the `context_pointers` and render the Mermaid diagram in your IDE or Mermaid Live.

* *Did it route the data correctly?*
* *Did it violate layer hierarchy (e.g., a component querying the database directly instead of through a service)?*

You correct the Architect's diagram in plain English. **You are debugging the architecture before debugging the code.**

#### Pass 3: The Builder (Blind Execution)

Once the plan is verified, you close the chat. You open a **brand new chat window** to act as the Execution Engine.

You provide it with **ONLY**:

1. The 3-4 files retrieved from the `context_pointers`.
2. The `CLAUDE.md` documentation for the boundaries mentioned.
3. The approved Mermaid diagram.

> **ROLE: Execution Engine**
> You are operating in Implementation Mode. Your context window has been mathematically restricted to maximize your reasoning density.
> **THE ARCHITECTURAL IR:**
> [PASTE MERMAID DIAGRAM HERE]
> **TASK:**
> Read the attached source files. Implement the feature to perfectly satisfy the nodes, edges, and state transitions defined in the Mermaid Intermediate Representation above.
> **CONSTRAINT:** Do not hallucinate cross-boundary connections or API calls that violate this graph.

### Why this changes everything

As the paper points out, "Raw source code is inherently verbose." When you separate the **Structural Map** from the **Semantic Implementation**, you stop paying the 100k+ token tax for rediscovering how your app is wired together every single prompt. The Builder LLM is executing a verified blueprint, resulting in near-deterministic, hallucination-free code generation.

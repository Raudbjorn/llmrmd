# llmermaid

Indexer, planner, and interactive TUI for LLM-assisted monorepo development.

llmermaid treats **Mermaid diagrams as structural ground truth** -- a
deterministic intermediate representation that constrains LLM reasoning and
prevents architectural hallucinations. It scans your codebase, extracts and
indexes every diagram, and assembles minimal, token-budgeted planning context
so an LLM can reason about your system's architecture without drowning in raw
source code.

The core thesis: **planning and implementation should use different context
windows.** Planning needs breadth (system overview, architectural diagrams,
conventions). Implementation needs depth (specific files, patterns, exact
syntax). llmermaid separates these phases, producing a compact planning prompt
(~1,500--3,000 tokens) from repositories that would otherwise require 50,000+
tokens of raw code.

## How It Works

```
BUILD TIME                          PLANNING PHASE                    IMPLEMENTATION

Source Code ──→ llmermaid index      User: "Add locations table"       Fresh context window:
                  │                    │                                 - Plan (~300 tok)
                  ▼                    ▼                                 - Diagrams (~500 tok)
               .claude/              llmermaid plan                      - Conventions (~200 tok)
               ├── file-index.toon     ├── reads manifest.toon           ─────────────────
               ├── manifest.toon       ├── loads relevant diagrams       ~1,200 tokens total
               ├── boundaries.toon     ├── loads CLAUDE.md chain
               ├── edges.toon          └── outputs planner-context.md
               ├── semantic-index.json
               └── diagrams/extracted/*.mmd
```

**Token economy**: TOON (Token-Oriented Object Notation) format achieves ~40%
token reduction vs JSON for typical monorepos, and Mermaid diagrams compress
architectural intent by up to 80% vs raw source code.

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
# Binary at target/release/llmermaid
```

### Optional features

```bash
# Enable Anthropic API integration for automated plan generation
cargo build --release --features api

# Enable pixel-perfect SVG→PNG→Kitty diagram rendering
cargo build --release --features pixel-diagrams
```

### Requirements

- Rust 1.75+
- `claude` CLI (detected and offered for install on first run)

## Usage

### Interactive TUI

```bash
llmermaid
```

Launches a four-view terminal interface:

```
┌─ llmermaid ───────────────────────────────────────────────────┐
│  1:Index   2:Plan   3:Diagrams   4:Handoff                       │
├───────────────────────────────────────────────────────────────────┤
│ Domains (7)     │ Files -- web (142) [Tab:toggle] │ Diagrams (6) │
│                 │                                  │              │
│ > admin    (85) │  cmp src/routes/+page.svelte     │ -- System -- │
│   auth     (12) │  cmp src/routes/locations/…      │ [*] arch     │
│   infra    (23) │  mod src/lib/api.ts              │   (450 tok)  │
│   root      (8) │  mod src/lib/services/…          │              │
│   shared   (34) │  cfg svelte.config.ts            │ -- Class --  │
│   supabase (45) │  ai  CLAUDE.md                   │ [ ] auth-er  │
│ > web     (142) │  ...                             │   (280 tok)  │
│                 │                                  │              │
│                 │                                  │ Budget ██░ 3k│
├───────────────────────────────────────────────────────────────────┤
│ INDEX │ 342 files, 6 diagrams, 7 domains         ?:help q:quit   │
└───────────────────────────────────────────────────────────────────┘
```

| View | Key | Purpose |
|------|-----|---------|
| **Index** | `1` | Browse files by domain, view diagram summaries, manage token budget |
| **Plan** | `2` | Describe changes, select scopes, generate planning context |
| **Diagrams** | `3` | View ASCII-rendered Mermaid diagrams, run linter, edit inline |
| **Handoff** | `4` | Review generated prompt, run agent pipeline, export to `.claude/` |

Press `?` in any view for full keybindings.

### CLI Subcommands

```bash
# Index your repository (writes to .claude/)
llmermaid index
llmermaid index --dry-run      # Preview without writing

# Assemble planning context
llmermaid plan "add user authentication"
llmermaid plan "add auth" --scopes web,supabase --budget 5000
llmermaid plan --call-api      # Send to Anthropic API for automated planning
llmermaid plan --list          # List saved plans
llmermaid plan --show <id>     # Show a specific plan

# Search diagrams and files
llmermaid search "payment flow" --limit 10

# Install git pre-commit hook
llmermaid init-hook

# Sync diagrams to IDE context files
llmermaid sync-editor --target cursor     # .cursorrules
llmermaid sync-editor --target windsurf   # .windsurf-context
llmermaid sync-editor --target generic    # .ai-context.md

# Generate Mermaid gitGraph from git history
llmermaid git-graph --limit 50
```

### Global flags

```
--root <PATH>     Override repository root (default: current directory)
--verbose         Enable debug logging
--skip-checks     Skip startup preflight checks
```

## Key Concepts

### Mermaid as Intermediate Representation

LLMs are probabilistic engines that predict token sequences. Without
structural constraints, they hallucinate API calls and violate architectural
boundaries. Mermaid diagrams inject a deterministic "logic gate" into the
reasoning process -- if there's no edge between Service A and Database B in
the graph, the model is statistically less likely to hallucinate a direct
connection.

A Mermaid sequence diagram representing an OAuth2 handshake consumes ~200
tokens. The implementation code spans ~2,000 tokens across multiple files.
This 10x compression maximizes the "reasoning density" of the context window.

### Boundary Detection

llmermaid detects project boundaries by scanning for sentinel files:

| Boundary Type | Sentinel Files |
|---------------|---------------|
| Rust Crate | `Cargo.toml` |
| Node Package | `package.json` |
| SvelteKit App | `svelte.config.js` |
| Python Package | `pyproject.toml`, `setup.py` |
| Go Module | `go.mod` |
| Monorepo Root | `pnpm-workspace.yaml`, `turbo.json`, `nx.json` |
| Tauri App | `tauri.conf.json` |
| Docker | `Dockerfile`, `docker-compose.yml` |
| Supabase | `supabase/config.toml` |
| Terraform | `main.tf` |

### Dependency Edges

Cross-domain dependencies are extracted from manifest files (`package.json`,
`Cargo.toml`, `pyproject.toml`, `go.mod`) and cross-referenced against known
domain names. This produces a dependency graph that the planner uses to
include transitively relevant context.

### TOON Format

Token-Oriented Object Notation -- a compact tabular format designed for LLM
consumption:

```
path            | domain | type      | tokens
src/lib.rs      | core   | module    | 45
src/main.rs     | core   | module    | 120
src/auth/mod.rs | auth   | component | 230
```

For a 500-file monorepo:

| Format | Tokens | Ratio |
|--------|--------|-------|
| Raw JSON file index | ~8,000 | 1.0x |
| TOON tabular format | ~3,200 | **0.40x** |
| Planning context (typical) | ~1,800 | -- |
| Implementation handoff | ~1,200 | -- |

### Progressive Disclosure via CLAUDE.md

The monorepo structure provides natural scoping. Each `CLAUDE.md` is a context
boundary -- it tells the LLM everything it needs at that scope, plus pointers
to adjacent scopes:

```
./CLAUDE.md                     <- System overview + cross-references
./apps/web/CLAUDE.md            <- Web app conventions + pointers to shared/supabase
./apps/admin/CLAUDE.md          <- Admin conventions + pointer to supabase/migrations
./packages/shared/CLAUDE.md     <- Shared types/utils conventions
```

llmermaid discovers and chains these files, including only the ones relevant
to the inferred scope of the user's change.

### Agent Pipeline

The DAG executor implements a Plan-Verify-Execute workflow as a directed
acyclic graph with topological ordering (Kahn's algorithm):

```
[Generate IR] --> [Validate Syntax] --> [Check Consistency] --> [Gate] --> [Execute]
```

Each node has a pluggable `StepExecutor`. The `Gate` node pauses for human
approval before proceeding. Pre-built graphs exist for planning
(`default_plan_graph`) and repair (`repair_graph`). The Handoff view
visualizes pipeline progress and provides interactive gate approval.

### Hybrid Search

`llmermaid search` combines three scoring signals:

1. **Field boost** -- exact/partial term matches on diagram ID, scope, type,
   source file (weighted by field importance)
2. **Description boost** -- NL description term overlap
3. **TF-IDF cosine similarity** -- structural similarity from the semantic
   index

Degrades gracefully when no semantic index is present.

### Mermaid Repair

The `repair.rs` module applies heuristic auto-fix passes to extracted Mermaid
syntax:

- Parentheses escaping in node labels
- Arrow syntax normalization
- ER diagram brace cardinality fixes
- Duplicate node ID resolution
- Subgraph nesting validation

This implements the "Repairer Pattern" from the research: a self-healing loop
that validates and auto-fixes LLM-generated or stale Mermaid before it enters
the index.

## Architecture

```
src/
├── main.rs              CLI dispatcher (clap)
├── lib.rs               Crate root (13 public modules)
├── startup.rs           Preflight: binary detection, auth verification
├── config.rs            Compile-time constants, skip lists, type maps
├── settings.rs          Runtime config from .llmermaid.toml
├── error.rs             Unified error types
│
├── indexer/             Core scanning engine
│   ├── mod.rs           scan_repo(), write_index(), TOON renderers
│   ├── types.rs         FileRecord, DiagramRecord, IndexResult
│   ├── mermaid.rs       Extraction, minification, type inference
│   ├── repair.rs        Heuristic Mermaid syntax auto-repair
│   ├── description.rs   Natural-language diagram descriptions
│   ├── toon.rs          TOON tabular format serializer
│   ├── boundary.rs      Project boundary detection (10 types)
│   └── edges.rs         Cross-domain dependency edges (4 manifest formats)
│
├── planner/             Context assembly
│   ├── mod.rs           select_context(), render_planning_prompt()
│   ├── types.rs         PlannerContext, ManifestEntry
│   ├── scope.rs         Keyword -> domain inference
│   ├── injection.rs     Hierarchical context injection
│   ├── tags.rs          Semantic XML tag wrapping
│   └── plans.rs         Plan persistence (save/load/approve)
│
├── agent/               DAG execution engine
│   ├── mod.rs           ExecutionGraph, topological sort (Kahn's algorithm)
│   └── executor.rs      GraphExecutor, StepExecutor trait, state machine
│
├── api/                 Anthropic API bridge
│   ├── mod.rs           build_plan_request(), parse_plan_response()
│   ├── client.rs        HTTP client (feature-gated)
│   ├── sanitize.rs      Secret scrubbing
│   ├── tool_schema.rs   submit_plans tool definition
│   └── validate.rs      Plan validation against file/manifest
│
├── graphrag/            Structural RAG
│   ├── index.rs         TF-IDF index build/load/search
│   └── tfidf.rs         Vectorizer and cosine similarity
│
├── search.rs            Hybrid search (field-boost + TF-IDF)
├── plugins/             AST -> Mermaid plugin system
│   ├── mod.rs           LanguagePlugin trait
│   ├── registry.rs      Plugin discovery and dispatch
│   ├── tiers.rs         C4-tier generation helpers
│   └── builtin.rs       Built-in language plugins
│
├── tui/                 Interactive terminal UI (TEA pattern)
│   ├── mod.rs           Main loop, external editor integration
│   ├── app.rs           App state machine (sole mutation point)
│   ├── message.rs       Message enum
│   ├── input.rs         KeyEvent -> Message (pure function)
│   ├── views/           Render functions (Index, Plan, Diagram, Handoff)
│   └── widgets/         Shared widget helpers
│
├── cli.rs               Claude CLI wrapper (blocking + streaming)
├── hooks.rs             Git pre-commit hook management
├── editor_sync.rs       IDE context file sync (Cursor/Windsurf/generic)
└── git_graph.rs         Git history -> Mermaid gitGraph
```

## Configuration

### Project-level: `.llmermaid.toml`

```toml
[general]
token_budget = 4000

[indexer]
skip_dirs = ["vendor", "dist"]

[planner]
default_scopes = ["web", "api"]

[plugins]
enabled = ["rust", "typescript"]
```

### User-level: `~/.config/llmermaid/config.toml`

Same schema as project-level; project settings take precedence.

## Documentation

- [Design Document](docs/DESIGN.md) -- architecture, workflow, rationale
- [Research Paper](docs/research.md) -- theoretical framework and tooling survey
- [v6 Context Compiler](docs/v6-context-compiler.md) -- boundary detection,
  subdomain classification, stable diagram IDs
- [Roadmap](docs/ROADMAP.md) -- gap analysis vs research goals, future improvements
- [Python PoC](docs/python-poc/) -- original proof-of-concept scripts

## Development

```bash
# Run tests (410 tests)
cargo test

# Run with debug logging
RUST_LOG=debug cargo run

# Check without building
cargo check

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt
```

## License

MIT

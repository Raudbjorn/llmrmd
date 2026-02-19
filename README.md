# llmermaid 🏛️

A single Rust binary that indexes monorepos and assembles minimal planning context for LLM-assisted development.

## The Problem

When using LLMs for monorepo development, you hit a fundamental chicken-and-egg problem:

> You need context to construct the right prompt, but you need the prompt to discover what context is relevant.

CLI tools fail here because they demand you already know what you're asking for:
```bash
# This requires knowing the scopes, the budget, the exact change wording...
llm-plan --root . --scopes "supabase,admin" --budget 4000 "Add a locations table"
```

## The Solution

An interactive TUI that lets you **explore → select → refine → export** in a natural flow:

```
┌─ llmermaid 🏛️ ────────────────────────────────────────────────┐
│  1:Index   2:Plan   3:Diagrams   4:Handoff                        │
├────────────────────────────────────────────────────────────────────┤
│ Domains (7)     │ Files — web (142) [Tab:toggle]                   │
│                 │                                                   │
│ ▸ admin    (85) │  🧩 src/routes/+page.svelte                      │
│   auth     (12) │  🧩 src/routes/locations/+page.svelte             │
│   infra    (23) │  📦 src/lib/api.ts                                │
│   root      (8) │  📦 src/lib/services/location.ts [services]       │
│   shared   (34) │  📦 src/lib/stores/map.ts [state]                 │
│   supabase (45) │  ⚙️ svelte.config.ts                              │
│ ▸ web     (142) │  📝 CLAUDE.md                                     │
│                 │  ...                                              │
├────────────────────────────────────────────────────────────────────┤
│ INDEX │ 342 files, 6 diagrams, 7 domains        ?:help q:quit     │
└────────────────────────────────────────────────────────────────────┘
```

## Architecture: Plan → Verify → Execute

```
Source Code ──→ llmermaid index ──→ .claude/
                                        ├── file-index.toon     (compact, ~40% of JSON)
                                        ├── manifest.toon       (diagram inventory)
                                        ├── boundaries.toon     (structural boundaries)
                                        ├── subdomains.toon     (semantic layers)
                                        ├── edges.toon          (dependency graph)
                                        └── diagrams/extracted/ (mermaid .mmd files)
                    │
                    ▼
            TUI: Browse → Describe change → Auto-infer scopes
                    │
                    ▼
            Planning context (~1500-3000 tokens)
            • System diagram (always)
            • Scope-relevant diagrams
            • Relevant CLAUDE.md conventions
            • Filtered file index
                    │
                    ▼
            Export → .claude/planner-context.md
            Feed to LLM planner (Pass 1: Architecture)
                    │
                    ▼
            Verified plan → Fresh LLM context (Pass 2: Implementation)
```

## Install

### From source (recommended for now)

```bash
cargo install --path .
```

### Arch Linux (AUR) — coming soon

```bash
paru -S llmermaid
```

## Usage

### Interactive TUI (default)

```bash
cd /path/to/monorepo
llmermaid
```

Keyboard shortcuts:
| Key | Action |
|-----|--------|
| `1-4` | Switch views (Index, Plan, Diagrams, Handoff) |
| `i` | Run/re-run indexer |
| `e` | Edit change description |
| `g` | Generate planning context |
| `x` | Export to `.claude/planner-context.md` |
| `h/j/k/l` | Navigate (vim-style) |
| `Tab` | Toggle all/filtered files |
| `q` / `Ctrl+C` | Quit |

### CLI mode

```bash
# Index a repo
llmermaid index --root /path/to/repo

# Generate planning context
llmermaid plan "Add a locations table with PostGIS geometry"

# With explicit scopes and budget
llmermaid plan --scopes "supabase,admin" --budget 6000 "Add auth middleware"

# Pipe to stdout
llmermaid plan --stdout "Refactor payment flow" | pbcopy
```

## Diagram Rendering

The TUI renders Mermaid diagrams as ASCII art directly in the terminal using
[graphs-tui](https://github.com/decisiongraph/graphs-tui):

```
 ┌─────┐     ┌───────┐     / \
 │Start│────▶│Process│────▶<Dec>
 └─────┘     └───────┘     \ /
                              │
                    ┌────Yes──┘
                    ▼
                  ┌────┐
                  │Done│
                  └────┘
```

Supports flowcharts, state diagrams, and pie charts in ASCII/Unicode.

## Token Economics

For a 500-file monorepo:

| Format | Tokens | Ratio |
|--------|--------|-------|
| Raw JSON file index | ~8,000 | 1.0× |
| TOON tabular format | ~3,200 | **0.40×** |
| Planning context (typical) | ~1,800 | — |
| Implementation handoff | ~1,200 | — |

The key insight: planning needs **breadth** (system overview, diagrams, conventions).
Implementation needs **depth** (specific files, patterns, exact syntax). By separating
these phases into different context windows, you avoid the "stuffed context" problem.

## Project Structure

```
src/
├── main.rs           CLI dispatch (clap)
├── lib.rs            Crate root
├── config.rs         Constants: skip dirs, extension maps, domain rules
├── error.rs          Unified error type (errors as values, no panics)
├── indexer/
│   ├── mod.rs        scan_repo(), write_index()
│   ├── types.rs      FileRecord, DiagramRecord, IndexResult
│   ├── mermaid.rs    Mermaid extraction & type inference
│   └── toon.rs       TOON tabular serialization
├── planner/
│   ├── mod.rs        select_context(), render_planning_prompt()
│   ├── types.rs      ManifestEntry, PlannerContext
│   ├── scope.rs      Scope inference (keyword matching)
│   └── planner_prompt.txt
└── tui/
    ├── mod.rs        Terminal setup, event loop
    ├── app.rs        App state machine
    ├── input.rs      Key bindings
    ├── views/
    │   ├── mod.rs    Top-level renderer + tabs + status bar
    │   ├── index.rs  Domain browser + file list
    │   ├── plan.rs   Change description + scope selection
    │   ├── diagram.rs ASCII diagram viewer
    │   └── handoff.rs Plan review + export
    └── widgets/
        └── mod.rs    (Reusable widgets — future extraction)
```

## License

MIT

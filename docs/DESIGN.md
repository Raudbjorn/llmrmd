# llmermaid: Design Document

## Overview

**llmermaid** is a two-tool system for LLM-assisted monorepo development:

1. **Indexer** (`llm-index`) — build-time, deterministic scan that produces a
   compact file index and diagram manifest in TOON format
2. **Planner** (`llm-plan`) — interactive context assembler that reads the
   indexer's artifacts and produces a minimal planning prompt for an LLM

The key insight: **planning and implementation should use different context
windows**. Planning needs breadth (system overview, architectural diagrams,
conventions). Implementation needs depth (specific files, patterns, exact
syntax). By separating these phases, we avoid the "stuffed context" problem
where an LLM has too much irrelevant information to reason effectively.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        BUILD TIME                               │
│                                                                 │
│   Source Code ──→ llm-index ──→ .claude/                       │
│                                  ├── file-index.toon            │
│                                  ├── file-index.json            │
│                                  ├── manifest.toon              │
│                                  ├── manifest.json              │
│                                  └── diagrams/extracted/*.mmd   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      PLANNING PHASE                             │
│                                                                 │
│   User: "Add a locations table"                                │
│     │                                                           │
│     ▼                                                           │
│   llm-plan ──reads──→ manifest.toon (tiny: ~200 tokens)       │
│     │        ──reads──→ file-index.json                        │
│     │        ──loads──→ relevant diagrams (selected, not all)  │
│     │        ──loads──→ relevant CLAUDE.md files               │
│     │                                                           │
│     ▼                                                           │
│   planner-context.md  (~1500-3000 tokens)                      │
│     │                                                           │
│     ▼                                                           │
│   LLM Planning Session ──→ Implementation Plan                 │
│                         ──→ Updated Diagrams                   │
│                         ──→ Context Pointers                   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   IMPLEMENTATION PHASE                           │
│                                                                 │
│   FRESH context window receives:                                │
│     • Implementation plan         (~300 tokens)                │
│     • Updated target diagrams     (~500 tokens)                │
│     • Context pointers            (~200 tokens)                │
│     • Convention summary          (~200 tokens)                │
│     ─────────────────────────────────────────                  │
│     Total: ~1200 tokens (vs 50,000+ for full codebase)         │
│                                                                 │
│   LLM implements step-by-step with maximum context budget      │
│   available for the actual code it needs to read/write.         │
└─────────────────────────────────────────────────────────────────┘
```

## Progressive Disclosure via CLAUDE.md Hierarchy

The monorepo structure itself provides natural scoping. Each CLAUDE.md is a
**context boundary** — it tells the LLM everything it needs at that scope,
plus pointers to adjacent scopes.

```
./CLAUDE.md                     ← System overview + cross-references
./apps/web/CLAUDE.md            ← Web app conventions + pointers to shared/supabase
./apps/admin/CLAUDE.md          ← Admin conventions + pointer to supabase/migrations
./packages/shared/CLAUDE.md     ← Shared types/utils conventions
./supabase/CLAUDE.md            ← Database conventions, migration naming, RLS policies
```

### Rules for CLAUDE.md content at each level

**Root CLAUDE.md** (always loaded):
- High-level system diagram (Mermaid, < 100 tokens)
- List of apps/packages with 1-line descriptions
- Cross-reference pointers: "database → apps/admin", "auth → packages/auth"
- Global conventions (commit format, branch strategy, etc.)

**App/Package CLAUDE.md** (loaded when scope matches):
- Component diagram for this app/package
- Directory structure explanation
- Key patterns with file references ("CRUD endpoints follow routes/api/users/")
- Cross-references OUT: where this scope's responsibilities end and another begins

**Infrastructure CLAUDE.md** (loaded for data/infra changes):
- Schema diagram (ERD)
- Migration naming conventions
- Environment-specific notes
- Deployment topology

### Cross-reference format

Use structured pointers that tools can parse:

```markdown
## Cross-references
| Domain      | Location                  | CLAUDE.md                    |
|-------------|---------------------------|------------------------------|
| Database    | `supabase/migrations/`    | `supabase/CLAUDE.md`         |
| Shared types| `packages/shared/src/`    | `packages/shared/CLAUDE.md`  |
| Auth        | `packages/auth/`          | `packages/auth/CLAUDE.md`    |
```

## TOON Format Usage

### Why TOON over JSON for the file index

The file index is a textbook case for TOON: a uniform array of objects with
identical primitive fields. For a 500-file monorepo:

| Format       | Tokens (est.) | Ratio  |
|-------------|---------------|--------|
| JSON         | ~8,000        | 1.0×   |
| JSON compact | ~5,500        | 0.69×  |
| TOON tabular | ~3,200        | 0.40×  |

The 60% saving matters because this index might be loaded in *every* planning
session. At 3,200 tokens, it's affordable; at 8,000, it's eating into the
budget for diagrams and conventions.

### File Index Format

```toon
# REPO FILE INDEX — generated 2025-02-18T12:00:00Z
# Root: massif-network

# CLAUDE.md locations (progressive disclosure chain)
claude_docs[4]{path}:
CLAUDE.md
apps/admin/CLAUDE.md
apps/web/CLAUDE.md
supabase/CLAUDE.md

# FILE INDEX
files[342]{path,type,domain,claude_md}:
apps/web/src/routes/+page.svelte,component,web,apps/web/CLAUDE.md
apps/web/src/lib/api.ts,module,web,apps/web/CLAUDE.md
apps/admin/src/routes/api/users/+server.ts,module,admin,apps/admin/CLAUDE.md
packages/shared/src/types/user.ts,module,shared,packages/shared/CLAUDE.md
supabase/migrations/20250101_init.sql,migration,supabase,supabase/CLAUDE.md
...
```

### Manifest Format

```toon
# DIAGRAM MANIFEST — generated 2025-02-18T12:00:00Z
# Use this to select which diagrams to load for planning.

# AVAILABLE DIAGRAMS (load selectively based on task scope)
diagrams[6]{source,scope,type,tokens}:
docs/system-overview.mmd,root,flowchart,82
apps/web/docs/architecture.mmd,web,flowchart,340
apps/admin/docs/architecture.mmd,admin,flowchart,290
supabase/docs/schema.mmd,supabase,erDiagram,480
packages/auth/docs/auth-flow.mmd,auth,sequence,210
apps/web/docs/payment-flow.mmd,web,sequence,180
```

## Planner Workflow

### Step 1: User describes the change

```bash
llm-plan "Add a 'locations' table with PostGIS geometry column,
          create a CRUD API in admin, and add a map view page"
```

### Step 2: Scope inference

The planner parses the description and matches keywords:
- "table" → supabase scope
- "API" + "admin" → admin scope
- "map view" → admin scope (or web, depending on context)

### Step 3: Context selection

From the manifest, the planner selects:
- System diagram (always, ~82 tokens)
- `supabase/docs/schema.mmd` (480 tokens) — because we're adding a table
- `apps/admin/docs/architecture.mmd` (290 tokens) — because we're adding an API

From CLAUDE.md files:
- `CLAUDE.md` (root conventions)
- `supabase/CLAUDE.md` (migration naming)
- `apps/admin/CLAUDE.md` (API patterns)

File index: filtered to only supabase + admin domains.

### Step 4: Planning prompt generated

Total context: ~1,800 tokens. The LLM has:
- A clear view of the system architecture
- Detailed diagrams of the affected components
- Naming conventions and patterns to follow
- A focused file listing showing what exists

### Step 5: LLM produces the plan

The planner LLM outputs:
1. Updated schema diagram (with new `locations` table)
2. Updated admin architecture diagram (with new CRUD endpoints)
3. Implementation steps (create migration, add types, add API route, add page)
4. Context pointers (which files the implementer needs)

### Step 6: Handoff to implementation

The plan + updated diagrams + context pointers are assembled into a
~1,200 token handoff document. A **fresh** LLM context receives this
and has nearly its entire context window available for the actual code.

## Integration with CI/CD

### Git Hook (recommended for solo/small team)

```bash
#!/bin/bash
# .git/hooks/pre-commit
if git diff --cached --name-only | grep -qE '\.(ts|svelte|py|sql|mmd)$'; then
    llm-index --root .
    git add .claude/
fi
```

### GitHub Action (recommended for teams)

```yaml
name: Update LLM Index
on:
  push:
    branches: [main, develop]
    paths:
      - 'apps/**'
      - 'packages/**'
      - 'supabase/**'
      - '*.mmd'
      - 'CLAUDE.md'

jobs:
  index:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v4
      - run: uv tool install llmermaid
      - run: llm-index --root .
      - uses: stefanzweifel/git-auto-commit-action@v5
        with:
          commit_message: "chore: update LLM file index"
          file_pattern: ".claude/*"
```

## Future Enhancements

### Automatic Mermaid generation from AST

For TypeScript/SvelteKit projects:
- **TsUML2** for class/interface diagrams from TypeScript AST
- **Tree-sitter** for extracting route → handler → service call chains
- Custom SvelteKit route scanner for page/API route topology

### Structural RAG

Index the diagrams themselves into a vector database alongside natural language
descriptions. When a user asks "How is payment processed?", retrieve the
sequence diagram for the payment flow.

### Multi-plan comparison

Have the planner generate 2-3 alternative implementation approaches (e.g.,
"minimal change" vs "full refactor" vs "feature-flagged rollout") and present
them side-by-side with token cost estimates.

### Automatic plan validation

After the implementer LLM writes code, re-run the indexer and diff the
generated diagrams against the "target state" diagrams from the plan. If they
diverge, flag the discrepancy.

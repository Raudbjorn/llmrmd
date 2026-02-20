# Roadmap: Closing the Gap Between Research Vision and Implementation

This document maps the goals defined in [research.md](research.md) and the
[v6 Context Compiler](v6-context-compiler.md) design against the current Rust
implementation, identifies what's missing, and proposes concrete next steps.

The evaluation draws from a multi-version comparison (v1--v4) of the codebase.
The current `main` branch represents v4 (baseline) plus cherry-picked features
from v1 (boundary/edge detection), v2 (plugin system, settings, editor sync),
and v3 (DAG executor, TF-IDF search, iterative repair).

---

## Status Matrix

| # | Goal | Status | Gap |
|---|------|--------|-----|
| G1 | Mermaid as IR | Done | -- |
| G2 | Plan-Verify-Execute workflow | Partial | TUI has plan + handoff but no phased Describe/Verify/Execute flow |
| G3 | Automated AST generation | Partial | Plugin trait + registry exist; no builtin generators invoke external tools yet |
| G4 | Hierarchical L1/L2/L3 summarization | Partial | Budget-aware selection exists; no formal C4 tier weights |
| G5 | Context-as-Code pipeline | Partial | Git hooks + editor sync done; no GitHub Actions generation |
| G6 | Repairer pattern | Done | `repair.rs` with heuristic auto-fix passes |
| G7 | LLMermaid agentic pattern | Done | DAG executor with Kahn's algorithm, pluggable `StepExecutor`, pre-built graphs |
| G8 | Structural RAG / GraphRAG | Partial | TF-IDF search + field boosts; no vector DB or embedding model |
| G9 | Boundary detection + dependency edges | Done | 10 boundary types, 4 manifest formats, wired into indexer + TUI |
| G10 | TOON token economy | Done | -- |
| G11 | Stable diagram IDs | Partial | ID extraction exists; `<!-- id: name -->` convention not enforced |
| G12 | CLAUDE.md progressive disclosure | Done | -- |
| G13 | Semantic diagram tags | Partial | `tags.rs` + `injection.rs` exist; not integrated into prompt rendering |
| G14 | Context-aware injection | Done | Scope-based selection with budget constraints |
| G15 | Scope inference | Done | Keyword-to-domain mapping |

---

## Priority 1: Core Workflow Gaps

### P1.1 -- Plan-Verify-Execute TUI Phases (G2)

**What the research says**: The "holy grail" is separating planning (breadth)
from implementation (depth) using distinct context windows, with a human
verification gate between them (Sec 2.3, 6.1).

**Current state**: The TUI has a Plan view (describe change + select scopes)
and a Handoff view (export prompt + agent pipeline), but no explicit phased
workflow where the user reviews the LLM's plan output before implementation
begins.

**Proposed work**:
- Add a `Verify` phase to the Plan view that displays the LLM's structured
  plan response (from `--call-api`) and lets the user approve, reject, or
  edit individual plan items inline
- Wire the DAG executor's `Gate` node type to pause between plan generation
  and implementation, requiring explicit user approval
- Show diff-style before/after of proposed diagram changes during verification

### P1.2 -- C4-Tier Hierarchical Injection (G4)

**What the research says**: Maintain diagrams at L1 (System, always included),
L2 (Container, for arch queries), L3 (Class, for module edits) with explicit
token weights (Sec 5.2).

**Current state**: `planner/tags.rs` defines `DiagramTag` variants and
`injection.rs` has hierarchy helpers, but `select_context()` in
`planner/mod.rs` uses flat smallest-first greedy selection without tier
awareness.

**Proposed work**:
- Classify diagrams into C4 tiers using `diagram_type` (already inferred)
- Apply tier weights: System 0.15, Container 0.35, Class 0.50 of token budget
- Always include L1 system diagrams (never dropped under budget pressure)
- Integrate `injection.rs` hierarchy into the main `select_context()` path

### P1.3 -- Semantic Diagram Tag Integration (G13)

**What the research says**: Wrap diagrams in XML tags with scope/level/priority
attributes for LLM compartmentalization (Sec 5.1).

**Current state**: `planner/tags.rs` defines 8 `DiagramTag` variants with XML
attributes, but `render_planning_prompt()` still uses markdown fences.

**Proposed work**:
- Replace markdown fences in `render_planning_prompt()` with XML-wrapped
  output from `tags.rs`
- Emit `<system_architecture_diagram scope="web" tokens="150">` wrappers
- Add explicit instruction in the system prompt telling the LLM how to use
  the tagged diagrams

---

## Priority 2: Automation and Integration

### P2.1 -- GitHub Actions CI/CD Generation (G5)

**What the research says**: Diagrams should be build artifacts regenerated on
every push, with the CI pipeline committing updated artifacts back to the repo
(Sec 4.2).

**Proposed work**:
- Add `llmermaid init-ci` subcommand that generates a
  `.github/workflows/llmermaid.yml` workflow file
- Workflow: on push to main, run `llmermaid index`, commit `.claude/` updates
- Optional: diff-check that fails the PR if diagrams are stale

### P2.2 -- Builtin AST Generators (G3)

**What the research says**: Manual documentation rots; diagrams must be
generated programmatically from AST (Sec 3).

**Current state**: `plugins/` module defines `LanguagePlugin` trait with
`generate()` method, `PluginRegistry` for dispatch by extension, and builtin
stubs -- but no generator actually invokes an external tool or parses AST.

**Proposed work**:
- Implement `RustPlugin` using `syn` crate to extract module/struct/impl
  relationships as class diagrams
- Implement `TypeScriptPlugin` that shells out to `tsuml2` if available
- Implement `PythonPlugin` that shells out to `pymermaider` if available
- Each plugin: `check_prerequisites() -> bool` detects tool availability,
  `generate()` returns `Vec<GeneratedDiagram>` merged into the manifest

### P2.3 -- Pipeline Integration: stdin/stdout Mode

**What the PoC had**: `echo "add auth" | llm-plan --stdout | pbcopy` -- Unix
pipeline composability.

**Current state**: All Rust versions are TUI-only or write to files. The `plan`
subcommand has `--stdout` but it's not wired for stdin piping.

**Proposed work**:
- Detect stdin is not a TTY: read change description from stdin
- With `--stdout`: write planning prompt to stdout instead of
  `.claude/planner-context.md`
- Enables: `echo "refactor auth" | llmermaid plan --stdout | claude -p -`

---

## Priority 3: Advanced Features

### P3.1 -- Graph-Aware Scope Inference (G15 enhancement)

**What the research says**: Use edge traversal for transitive dependencies
(v7-design Sec 7.2). If changing `auth`, also include `payments` because
`edges.toon` shows `payments -> auth`.

**Current state**: Scope inference uses keyword matching only (`scope.rs`).
Edges are now extracted and available in the indexer output.

**Proposed work**:
- After keyword-based scope inference, load `edges.toon` and traverse 1-hop
  neighbors of matched domains
- Add inferred transitive scopes with lower priority weight
- Display edge-inferred scopes distinctly in the Plan view

### P3.2 -- Stable Diagram IDs (G11)

**What the research says**: `<!-- id: auth-flow -->` HTML comments before
mermaid fences enable deterministic targeting (v6-context-compiler).

**Current state**: Diagram IDs are derived from filename + block index, which
is fragile across edits.

**Proposed work**:
- During extraction, detect `<!-- id: name -->` comments preceding fences
- Use explicit ID when present, fall back to current derivation
- Add `llmermaid id-check` subcommand that warns about diagrams missing
  explicit IDs

### P3.3 -- Subdomain Classification (subdomains.toon)

**What the v6 design says**: Bridge structural boundaries (where files live)
with semantic layers (what role files play): routes, components, services,
repositories, state, clients, utils.

**Current state**: `config.rs` defines `SUBDOMAIN_RULES` with 7 regex patterns
for semantic layer classification. `FileRecord` has a `subdomain` field that's
populated during indexing. But `subdomains.toon` is never emitted as a
standalone artifact.

**Proposed work**:
- Add `render_subdomain_toon()` to `indexer/mod.rs`
- Emit `subdomains.toon` alongside other artifacts in `write_index()`
- Wire subdomain data into scope inference (files in `services` layer are
  more architecturally significant than files in `utils`)

### P3.4 -- Layer Policy Enforcement

**What the research says**: Enforce dependency direction rules:
`routes -> components -> services -> repositories -> clients -> state`
(second-approach.md).

**Current state**: `api/tool_schema.rs` references layer policies in the
`submit_plans` tool definition, but no runtime enforcement exists.

**Proposed work**:
- Define layer ordering in `config.rs`
- During plan validation (`api/validate.rs`), check that proposed changes
  don't introduce upward dependencies (e.g., a repository importing a route)
- Emit warnings in the Plan view when edge violations are detected

### P3.5 -- Structural RAG with Embeddings (G8)

**Current state**: `graphrag/` has TF-IDF vectorization and cosine similarity.

**Proposed work**:
- Add optional `embedding` feature flag with `fastembed` or `candle` crate
- Generate dense embeddings for diagram NL descriptions
- Hybrid retrieval: TF-IDF for keyword precision + embedding for semantic
  recall
- Store in a local SQLite or LMDB index for persistence

### P3.6 -- Diagram Complexity Metrics

**What the research says**: Use complexity as a refactoring signal -- "a messy
diagram is a symptom of messy architecture" (Sec 7).

**Proposed work**:
- Count nodes, edges, and estimate cyclomatic complexity from parsed Mermaid
- Add `complexity_score` field to `DiagramRecord`
- Display complexity in the TUI's diagram summary panel
- Warn in the linter when complexity exceeds a configurable threshold

### P3.7 -- File-to-Diagram Reverse Mapping

**What the research says**: Enable context-aware injection where editing
`auth_service.py` automatically loads `auth_sequence_diagram.mmd` (Sec 5.2).

**Proposed work**:
- During indexing, build a reverse map: `file_path -> [diagram_ids]`
- Emit `file-diagram-map.json` in `.claude/`
- Wire into editor sync: when the IDE reports the active file, automatically
  select relevant diagrams

---

## Implementation Sequence

For maximum impact with minimum risk, the recommended order is:

1. **P1.2** (C4 tiers) -- improves prompt quality immediately, low blast radius
2. **P1.3** (XML tags) -- complements tier work, small change to prompt renderer
3. **P2.3** (stdin/stdout) -- unlocks pipeline composability, small scope
4. **P3.1** (graph-aware scopes) -- leverages existing edge data
5. **P3.3** (subdomains.toon) -- infrastructure already exists in config.rs
6. **P1.1** (Plan-Verify-Execute) -- largest effort, highest research alignment
7. **P2.2** (AST generators) -- high value but depends on external tooling
8. **P2.1** (GitHub Actions) -- CI/CD generation, moderate effort
9. Remaining P3 items by interest/need

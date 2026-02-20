//! llmermaid: Indexer + Planner + Interactive TUI for LLM-assisted monorepo development.
//!
//! # Architecture
//!
//! The tool implements a "Plan-Verify-Execute" workflow:
//!
//! 1. **Indexer** — scans a monorepo, produces a compact file index and diagram manifest
//! 2. **Planner** — reads indexer artifacts, assembles minimal planning context for an LLM
//! 3. **TUI** — interactive interface for browsing, selecting scopes, and generating plans
//!
//! Planning and implementation use *different* context windows. Planning needs breadth
//! (system overview, diagrams, conventions). Implementation needs depth (specific files,
//! patterns, exact syntax). This separation avoids the "stuffed context" problem.

pub mod agent;
pub mod api;
pub mod cli;
pub mod config;
pub mod editor_sync;
pub mod error;
pub mod git_graph;
pub mod graphrag;
pub mod hooks;
pub mod indexer;
pub mod planner;
pub mod plugins;
pub mod search;
pub mod settings;
pub mod tui;

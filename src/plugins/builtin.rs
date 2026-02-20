//! Built-in language plugins for common languages.
//!
//! Each plugin checks for external tooling via `which::which()` and degrades
//! gracefully to structural analysis when the preferred tool is absent.
//!
//! Current plugins:
//! - **Rust** -- analyzes `mod` / `use` declarations to build module dependency graphs
//! - **TypeScript** -- uses `tsuml2` if available, falls back to import analysis
//! - **Python** -- uses `pymermaider` if available, falls back to import analysis
//! - **Go** -- uses `go list` for package dependency graphs

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use tracing::debug;

use super::{DiagramLevel, GeneratedDiagram, LanguagePlugin};
use crate::error::{Error, Result};

// =============================================================================
// Rust
// =============================================================================

/// Rust language plugin -- analyzes module structure via `mod` and `use` statements.
///
/// Does not require external tools; reads source files directly.
/// Optional: `cargo` is checked as a signal that this is a Rust project.
pub struct RustPlugin;

impl LanguagePlugin for RustPlugin {
    fn name(&self) -> &str {
        "rust"
    }

    fn extensions(&self) -> &[&str] {
        &[".rs"]
    }

    fn check_prerequisites(&self) -> Result<bool> {
        // Rust plugin does pure source analysis; cargo is nice-to-have, not required.
        // We check for it as a signal that the environment is Rust-aware.
        Ok(which::which("cargo").is_ok())
    }

    fn generate(&self, root: &Path, files: &[&Path]) -> Result<Vec<GeneratedDiagram>> {
        let mut diagrams = Vec::new();

        let content = analyze_rust_modules(root, files)?;
        if !content.is_empty() {
            diagrams.push(GeneratedDiagram {
                content,
                level: DiagramLevel::Container,
                diagram_type: "flowchart".to_string(),
                label: "Rust Module Dependencies".to_string(),
            });
        }

        Ok(diagrams)
    }

    fn supported_levels(&self) -> Vec<DiagramLevel> {
        vec![DiagramLevel::Container, DiagramLevel::Class]
    }
}

/// Analyze Rust source files for `mod` declarations and `use crate::` imports
/// to build a module-level dependency flowchart.
fn analyze_rust_modules(root: &Path, files: &[&Path]) -> Result<String> {
    // module_name -> set of modules it depends on
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut known_modules: BTreeSet<String> = BTreeSet::new();

    for rel_path in files {
        let abs_path = root.join(rel_path);
        let content = match std::fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(e) => {
                debug!(path = %abs_path.display(), error = %e, "Skipping unreadable file");
                continue;
            }
        };

        let module_name = module_name_from_path(rel_path);
        known_modules.insert(module_name.clone());
        let entry = deps.entry(module_name).or_default();

        for line in content.lines() {
            let trimmed = line.trim();

            // `use crate::foo::bar` => dependency on "foo"
            if let Some(rest) = trimmed.strip_prefix("use crate::") {
                if let Some(top_mod) = rest.split("::").next() {
                    let top_mod = top_mod.trim_end_matches(';').to_string();
                    if !top_mod.is_empty() {
                        entry.insert(top_mod);
                    }
                }
            }
        }
    }

    if known_modules.is_empty() {
        return Ok(String::new());
    }

    // Build flowchart from known inter-module edges.
    let mut lines = vec!["flowchart TD".to_string()];

    for module in &known_modules {
        let safe_id = sanitize_mermaid_id(module);
        lines.push(format!("    {safe_id}[{module}]"));
    }

    let mut edge_count = 0;
    for (from, to_set) in &deps {
        let from_id = sanitize_mermaid_id(from);
        for to in to_set {
            if known_modules.contains(to) && from != to {
                let to_id = sanitize_mermaid_id(to);
                lines.push(format!("    {from_id} --> {to_id}"));
                edge_count += 1;
            }
        }
    }

    // If there are no edges, the diagram is not useful.
    if edge_count == 0 {
        return Ok(String::new());
    }

    Ok(lines.join("\n"))
}

/// Derive a module name from a relative file path.
///
/// `src/indexer/mod.rs` -> `indexer`
/// `src/config.rs`      -> `config`
/// `src/lib.rs`         -> `lib`
fn module_name_from_path(rel_path: &Path) -> String {
    let stem = rel_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    if stem == "mod" {
        // Use parent directory name.
        rel_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("mod")
            .to_string()
    } else {
        stem.to_string()
    }
}

// =============================================================================
// TypeScript
// =============================================================================

/// TypeScript / JavaScript plugin -- uses `tsuml2` for class diagrams if available,
/// otherwise performs basic import analysis.
pub struct TypeScriptPlugin;

impl LanguagePlugin for TypeScriptPlugin {
    fn name(&self) -> &str {
        "typescript"
    }

    fn extensions(&self) -> &[&str] {
        &[".ts", ".tsx", ".js", ".jsx"]
    }

    fn check_prerequisites(&self) -> Result<bool> {
        // We can do basic import analysis without any tools.
        // tsuml2 is optional for richer class diagrams.
        Ok(which::which("node").is_ok())
    }

    fn generate(&self, root: &Path, files: &[&Path]) -> Result<Vec<GeneratedDiagram>> {
        let mut diagrams = Vec::new();

        // Try tsuml2 for class diagrams first.
        if which::which("npx").is_ok() {
            match run_tsuml2(root, files) {
                Ok(Some(content)) => {
                    diagrams.push(GeneratedDiagram {
                        content,
                        level: DiagramLevel::Class,
                        diagram_type: "classDiagram".to_string(),
                        label: "TypeScript Class Diagram (tsuml2)".to_string(),
                    });
                }
                Ok(None) => {
                    debug!("tsuml2 produced no output");
                }
                Err(e) => {
                    debug!(error = %e, "tsuml2 failed, falling back to import analysis");
                }
            }
        }

        // Always do basic import analysis for module-level view.
        let content = analyze_ts_imports(root, files)?;
        if !content.is_empty() {
            diagrams.push(GeneratedDiagram {
                content,
                level: DiagramLevel::Container,
                diagram_type: "flowchart".to_string(),
                label: "TypeScript Module Dependencies".to_string(),
            });
        }

        Ok(diagrams)
    }

    fn supported_levels(&self) -> Vec<DiagramLevel> {
        vec![DiagramLevel::Container, DiagramLevel::Class]
    }
}

/// Attempt to run `npx tsuml2` on the given files.
fn run_tsuml2(root: &Path, files: &[&Path]) -> Result<Option<String>> {
    // tsuml2 expects glob patterns or specific files.
    let file_args: Vec<String> = files
        .iter()
        .take(50) // Avoid command-line length limits.
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    if file_args.is_empty() {
        return Ok(None);
    }

    let output = Command::new("npx")
        .args(["tsuml2", "--glob"])
        .args(&file_args)
        .arg("--format")
        .arg("mermaid")
        .current_dir(root)
        .output()
        .map_err(|e| Error::Plugin {
            plugin: "typescript".to_string(),
            message: format!("Failed to run tsuml2: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!(stderr = %stderr, "tsuml2 returned non-zero exit code");
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        Ok(None)
    } else {
        Ok(Some(stdout))
    }
}

/// Analyze TypeScript/JavaScript files for import statements.
fn analyze_ts_imports(root: &Path, files: &[&Path]) -> Result<String> {
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut known_modules: BTreeSet<String> = BTreeSet::new();

    for rel_path in files {
        let abs_path = root.join(rel_path);
        let content = match std::fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(e) => {
                debug!(path = %abs_path.display(), error = %e, "Skipping unreadable file");
                continue;
            }
        };

        let module_name = ts_module_name(rel_path);
        known_modules.insert(module_name.clone());
        let entry = deps.entry(module_name).or_default();

        for line in content.lines() {
            let trimmed = line.trim();
            // Match: import ... from './foo' or import ... from '../bar'
            // Also: import('./foo') dynamic imports
            if let Some(target) = extract_relative_import(trimmed) {
                entry.insert(target);
            }
        }
    }

    if known_modules.is_empty() {
        return Ok(String::new());
    }

    let mut lines = vec!["flowchart TD".to_string()];

    for module in &known_modules {
        let safe_id = sanitize_mermaid_id(module);
        lines.push(format!("    {safe_id}[{module}]"));
    }

    let mut edge_count = 0;
    for (from, to_set) in &deps {
        let from_id = sanitize_mermaid_id(from);
        for to in to_set {
            if known_modules.contains(to) && from != to {
                let to_id = sanitize_mermaid_id(to);
                lines.push(format!("    {from_id} --> {to_id}"));
                edge_count += 1;
            }
        }
    }

    if edge_count == 0 {
        return Ok(String::new());
    }

    Ok(lines.join("\n"))
}

/// Extract the target module name from a relative import statement.
///
/// Returns `None` for non-relative or unparseable imports.
fn extract_relative_import(line: &str) -> Option<String> {
    // Patterns: `from './foo'`, `from "../bar"`, `import('./baz')`
    let from_idx = line.find("from ")?;
    let after_from = &line[from_idx + 5..];
    let quote_char = after_from.chars().find(|c| *c == '\'' || *c == '"')?;
    let start = after_from.find(quote_char)? + 1;
    let rest = &after_from[start..];
    let end = rest.find(quote_char)?;
    let path = &rest[..end];

    if !path.starts_with('.') {
        return None;
    }

    // Take the last path segment, strip extension.
    let last_segment = path.rsplit('/').next().unwrap_or(path);
    let name = last_segment
        .strip_suffix(".ts")
        .or_else(|| last_segment.strip_suffix(".tsx"))
        .or_else(|| last_segment.strip_suffix(".js"))
        .or_else(|| last_segment.strip_suffix(".jsx"))
        .unwrap_or(last_segment);

    if name == "." || name == ".." || name.is_empty() {
        // Index re-export; use parent dir name from path.
        let parent = path.rsplit('/').nth(1)?;
        Some(parent.to_string())
    } else {
        Some(name.to_string())
    }
}

/// Derive a module name from a TS/JS file path.
fn ts_module_name(rel_path: &Path) -> String {
    let stem = rel_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    if stem == "index" {
        rel_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("index")
            .to_string()
    } else {
        stem.to_string()
    }
}

// =============================================================================
// Python
// =============================================================================

/// Python plugin -- uses `pymermaider` for class diagrams if available,
/// otherwise performs basic import analysis.
pub struct PythonPlugin;

impl LanguagePlugin for PythonPlugin {
    fn name(&self) -> &str {
        "python"
    }

    fn extensions(&self) -> &[&str] {
        &[".py", ".pyi"]
    }

    fn check_prerequisites(&self) -> Result<bool> {
        Ok(which::which("python3").is_ok() || which::which("python").is_ok())
    }

    fn generate(&self, root: &Path, files: &[&Path]) -> Result<Vec<GeneratedDiagram>> {
        let mut diagrams = Vec::new();

        // Try pymermaider for class diagrams.
        if which::which("pymermaider").is_ok() {
            match run_pymermaider(root, files) {
                Ok(Some(content)) => {
                    diagrams.push(GeneratedDiagram {
                        content,
                        level: DiagramLevel::Class,
                        diagram_type: "classDiagram".to_string(),
                        label: "Python Class Diagram (pymermaider)".to_string(),
                    });
                }
                Ok(None) => {
                    debug!("pymermaider produced no output");
                }
                Err(e) => {
                    debug!(error = %e, "pymermaider failed, falling back to import analysis");
                }
            }
        }

        // Basic import analysis for module-level view.
        let content = analyze_python_imports(root, files)?;
        if !content.is_empty() {
            diagrams.push(GeneratedDiagram {
                content,
                level: DiagramLevel::Container,
                diagram_type: "flowchart".to_string(),
                label: "Python Module Dependencies".to_string(),
            });
        }

        Ok(diagrams)
    }

    fn supported_levels(&self) -> Vec<DiagramLevel> {
        vec![DiagramLevel::Container, DiagramLevel::Class]
    }
}

/// Attempt to run `pymermaider` on the given files.
fn run_pymermaider(root: &Path, files: &[&Path]) -> Result<Option<String>> {
    let file_args: Vec<String> = files
        .iter()
        .take(50)
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    if file_args.is_empty() {
        return Ok(None);
    }

    let output = Command::new("pymermaider")
        .args(&file_args)
        .current_dir(root)
        .output()
        .map_err(|e| Error::Plugin {
            plugin: "python".to_string(),
            message: format!("Failed to run pymermaider: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!(stderr = %stderr, "pymermaider returned non-zero exit code");
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        Ok(None)
    } else {
        Ok(Some(stdout))
    }
}

/// Analyze Python files for import statements.
fn analyze_python_imports(root: &Path, files: &[&Path]) -> Result<String> {
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut known_modules: BTreeSet<String> = BTreeSet::new();

    for rel_path in files {
        let abs_path = root.join(rel_path);
        let content = match std::fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(e) => {
                debug!(path = %abs_path.display(), error = %e, "Skipping unreadable file");
                continue;
            }
        };

        let module_name = python_module_name(rel_path);
        known_modules.insert(module_name.clone());
        let entry = deps.entry(module_name).or_default();

        for line in content.lines() {
            let trimmed = line.trim();

            // `from .foo import bar` => relative import of "foo"
            if let Some(rest) = trimmed.strip_prefix("from .") {
                if let Some(top_mod) = rest.split_whitespace().next() {
                    let top_mod = top_mod.split('.').next().unwrap_or("").to_string();
                    if !top_mod.is_empty() && top_mod != "import" {
                        entry.insert(top_mod);
                    }
                }
            }
            // `from foo.bar import baz` => "foo"
            else if let Some(rest) = trimmed.strip_prefix("from ") {
                if let Some(top_mod) = rest.split('.').next() {
                    let top_mod = top_mod
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !top_mod.is_empty() && top_mod != "import" {
                        entry.insert(top_mod);
                    }
                }
            }
            // `import foo.bar` => "foo"
            else if let Some(rest) = trimmed.strip_prefix("import ") {
                if let Some(top_mod) = rest.split('.').next() {
                    let top_mod = top_mod
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !top_mod.is_empty() {
                        entry.insert(top_mod);
                    }
                }
            }
        }
    }

    if known_modules.is_empty() {
        return Ok(String::new());
    }

    let mut lines = vec!["flowchart TD".to_string()];

    for module in &known_modules {
        let safe_id = sanitize_mermaid_id(module);
        lines.push(format!("    {safe_id}[{module}]"));
    }

    let mut edge_count = 0;
    for (from, to_set) in &deps {
        let from_id = sanitize_mermaid_id(from);
        for to in to_set {
            if known_modules.contains(to) && from != to {
                let to_id = sanitize_mermaid_id(to);
                lines.push(format!("    {from_id} --> {to_id}"));
                edge_count += 1;
            }
        }
    }

    if edge_count == 0 {
        return Ok(String::new());
    }

    Ok(lines.join("\n"))
}

/// Derive a module name from a Python file path.
fn python_module_name(rel_path: &Path) -> String {
    let stem = rel_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    if stem == "__init__" {
        rel_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("__init__")
            .to_string()
    } else {
        stem.to_string()
    }
}

// =============================================================================
// Go
// =============================================================================

/// Go plugin -- uses `go list` for package dependency graphs.
pub struct GoPlugin;

impl LanguagePlugin for GoPlugin {
    fn name(&self) -> &str {
        "go"
    }

    fn extensions(&self) -> &[&str] {
        &[".go"]
    }

    fn check_prerequisites(&self) -> Result<bool> {
        Ok(which::which("go").is_ok())
    }

    fn generate(&self, root: &Path, files: &[&Path]) -> Result<Vec<GeneratedDiagram>> {
        let mut diagrams = Vec::new();

        // Try `go list` for package dependency graph.
        match run_go_list(root) {
            Ok(Some(content)) => {
                diagrams.push(GeneratedDiagram {
                    content,
                    level: DiagramLevel::Container,
                    diagram_type: "flowchart".to_string(),
                    label: "Go Package Dependencies".to_string(),
                });
            }
            Ok(None) => {
                debug!("go list produced no useful output");
            }
            Err(e) => {
                debug!(error = %e, "go list failed, falling back to import analysis");
            }
        }

        // Fallback: basic import analysis if go list failed or produced nothing.
        if diagrams.is_empty() {
            let content = analyze_go_imports(root, files)?;
            if !content.is_empty() {
                diagrams.push(GeneratedDiagram {
                    content,
                    level: DiagramLevel::Container,
                    diagram_type: "flowchart".to_string(),
                    label: "Go Package Dependencies (import analysis)".to_string(),
                });
            }
        }

        Ok(diagrams)
    }

    fn supported_levels(&self) -> Vec<DiagramLevel> {
        vec![DiagramLevel::Container]
    }
}

/// Run `go list -json ./...` and parse package dependencies.
fn run_go_list(root: &Path) -> Result<Option<String>> {
    // Check for go.mod to confirm this is a Go module.
    if !root.join("go.mod").exists() {
        return Ok(None);
    }

    let output = Command::new("go")
        .args(["list", "-f", "{{.ImportPath}}:{{join .Imports \",\"}}", "./..."])
        .current_dir(root)
        .output()
        .map_err(|e| Error::Plugin {
            plugin: "go".to_string(),
            message: format!("Failed to run go list: {e}"),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!(stderr = %stderr, "go list returned non-zero exit code");
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse `pkg:dep1,dep2,dep3` format.
    let mut packages: BTreeSet<String> = BTreeSet::new();
    let mut edges: Vec<(String, String)> = Vec::new();

    // Extract the module path from go.mod.
    let module_prefix = read_go_module_path(root).unwrap_or_default();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, ':').collect();
        if parts.is_empty() {
            continue;
        }

        let pkg = short_package_name(parts[0], &module_prefix);
        packages.insert(pkg.clone());

        if parts.len() > 1 {
            for dep in parts[1].split(',') {
                let dep = dep.trim();
                // Only track internal deps (within the module).
                if dep.starts_with(&module_prefix) && !dep.is_empty() {
                    let short_dep = short_package_name(dep, &module_prefix);
                    if short_dep != pkg {
                        edges.push((pkg.clone(), short_dep));
                    }
                }
            }
        }
    }

    if packages.is_empty() || edges.is_empty() {
        return Ok(None);
    }

    let mut lines = vec!["flowchart TD".to_string()];

    for pkg in &packages {
        let safe_id = sanitize_mermaid_id(pkg);
        lines.push(format!("    {safe_id}[{pkg}]"));
    }

    // Deduplicate edges.
    let unique_edges: BTreeSet<(String, String)> = edges.into_iter().collect();
    for (from, to) in &unique_edges {
        if packages.contains(to) {
            let from_id = sanitize_mermaid_id(from);
            let to_id = sanitize_mermaid_id(to);
            lines.push(format!("    {from_id} --> {to_id}"));
        }
    }

    Ok(Some(lines.join("\n")))
}

/// Read the module path from go.mod.
fn read_go_module_path(root: &Path) -> Option<String> {
    let go_mod = root.join("go.mod");
    let content = std::fs::read_to_string(go_mod).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("module ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Shorten a fully-qualified Go package name by removing the module prefix.
fn short_package_name(full: &str, module_prefix: &str) -> String {
    if module_prefix.is_empty() {
        return full.to_string();
    }
    full.strip_prefix(module_prefix)
        .and_then(|s| s.strip_prefix('/'))
        .unwrap_or(full)
        .to_string()
}

/// Analyze Go source files for import statements.
fn analyze_go_imports(root: &Path, files: &[&Path]) -> Result<String> {
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut known_packages: BTreeSet<String> = BTreeSet::new();

    for rel_path in files {
        let abs_path = root.join(rel_path);
        let content = match std::fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(e) => {
                debug!(path = %abs_path.display(), error = %e, "Skipping unreadable file");
                continue;
            }
        };

        // Use parent directory as package name (Go convention).
        let pkg_name = rel_path
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("main")
            .to_string();
        let pkg_name = if pkg_name.is_empty() {
            "main".to_string()
        } else {
            pkg_name.replace('\\', "/")
        };
        known_packages.insert(pkg_name.clone());

        let entry = deps.entry(pkg_name).or_default();

        let mut in_import_block = false;
        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("import (") {
                in_import_block = true;
                continue;
            }
            if in_import_block && trimmed == ")" {
                in_import_block = false;
                continue;
            }

            let import_path = if in_import_block {
                trimmed
                    .trim_matches('"')
                    .split_whitespace()
                    .last()
                    .map(|s| s.trim_matches('"'))
            } else {
                trimmed.strip_prefix("import ").map(|rest| rest.trim().trim_matches('"'))
            };

            if let Some(path) = import_path {
                // Only track short / relative-looking paths as internal deps.
                if !path.contains('.') && !path.is_empty() {
                    entry.insert(path.to_string());
                }
            }
        }
    }

    if known_packages.is_empty() {
        return Ok(String::new());
    }

    let mut lines = vec!["flowchart TD".to_string()];

    for pkg in &known_packages {
        let safe_id = sanitize_mermaid_id(pkg);
        lines.push(format!("    {safe_id}[{pkg}]"));
    }

    let mut edge_count = 0;
    for (from, to_set) in &deps {
        let from_id = sanitize_mermaid_id(from);
        for to in to_set {
            if known_packages.contains(to) && from != to {
                let to_id = sanitize_mermaid_id(to);
                lines.push(format!("    {from_id} --> {to_id}"));
                edge_count += 1;
            }
        }
    }

    if edge_count == 0 {
        return Ok(String::new());
    }

    Ok(lines.join("\n"))
}

// =============================================================================
// Shared helpers
// =============================================================================

/// Replace characters that are invalid in Mermaid node IDs with underscores.
fn sanitize_mermaid_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ---- Sanitize helper ----

    #[test]
    fn sanitize_mermaid_id_basic() {
        assert_eq!(sanitize_mermaid_id("hello"), "hello");
        assert_eq!(sanitize_mermaid_id("foo-bar"), "foo_bar");
        assert_eq!(sanitize_mermaid_id("src/lib"), "src_lib");
        assert_eq!(sanitize_mermaid_id("my.module"), "my_module");
    }

    // ---- Rust module name ----

    #[test]
    fn module_name_from_regular_file() {
        assert_eq!(
            module_name_from_path(Path::new("src/config.rs")),
            "config"
        );
    }

    #[test]
    fn module_name_from_mod_rs() {
        assert_eq!(
            module_name_from_path(Path::new("src/indexer/mod.rs")),
            "indexer"
        );
    }

    #[test]
    fn module_name_from_lib() {
        assert_eq!(module_name_from_path(Path::new("src/lib.rs")), "lib");
    }

    // ---- Rust module analysis ----

    #[test]
    fn analyze_rust_modules_empty() {
        let dir = tempfile::tempdir().unwrap();
        let result = analyze_rust_modules(dir.path(), &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn analyze_rust_modules_with_deps() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        std::fs::write(
            src.join("config.rs"),
            "pub const FOO: &str = \"bar\";\n",
        )
        .unwrap();
        std::fs::write(
            src.join("main.rs"),
            "use crate::config;\nfn main() {}\n",
        )
        .unwrap();

        let files: Vec<&Path> = vec![
            Path::new("src/config.rs"),
            Path::new("src/main.rs"),
        ];

        let result = analyze_rust_modules(dir.path(), &files).unwrap();
        assert!(result.contains("flowchart TD"));
        assert!(result.contains("main --> config"));
    }

    #[test]
    fn analyze_rust_modules_no_cross_deps() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        std::fs::write(src.join("a.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(src.join("b.rs"), "pub fn b() {}\n").unwrap();

        let files: Vec<&Path> = vec![Path::new("src/a.rs"), Path::new("src/b.rs")];

        let result = analyze_rust_modules(dir.path(), &files).unwrap();
        // No edges, so empty.
        assert!(result.is_empty());
    }

    // ---- TypeScript helpers ----

    #[test]
    fn ts_module_name_regular() {
        assert_eq!(ts_module_name(Path::new("src/utils.ts")), "utils");
    }

    #[test]
    fn ts_module_name_index() {
        assert_eq!(ts_module_name(Path::new("src/auth/index.ts")), "auth");
    }

    #[test]
    fn extract_relative_import_basic() {
        assert_eq!(
            extract_relative_import("import { foo } from './utils'"),
            Some("utils".to_string())
        );
    }

    #[test]
    fn extract_relative_import_with_extension() {
        assert_eq!(
            extract_relative_import("import { bar } from './helper.ts'"),
            Some("helper".to_string())
        );
    }

    #[test]
    fn extract_relative_import_parent() {
        assert_eq!(
            extract_relative_import("import { baz } from '../shared/types'"),
            Some("types".to_string())
        );
    }

    #[test]
    fn extract_relative_import_non_relative() {
        assert_eq!(
            extract_relative_import("import React from 'react'"),
            None
        );
    }

    #[test]
    fn extract_relative_import_double_quotes() {
        assert_eq!(
            extract_relative_import("import { x } from \"./config\""),
            Some("config".to_string())
        );
    }

    // ---- TypeScript import analysis ----

    #[test]
    fn analyze_ts_imports_with_deps() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        std::fs::write(src.join("config.ts"), "export const FOO = 'bar';\n").unwrap();
        std::fs::write(
            src.join("app.ts"),
            "import { FOO } from './config';\nconsole.log(FOO);\n",
        )
        .unwrap();

        let files: Vec<&Path> = vec![Path::new("src/config.ts"), Path::new("src/app.ts")];

        let result = analyze_ts_imports(dir.path(), &files).unwrap();
        assert!(result.contains("flowchart TD"));
        assert!(result.contains("app --> config"));
    }

    // ---- Python helpers ----

    #[test]
    fn python_module_name_regular() {
        assert_eq!(python_module_name(Path::new("src/utils.py")), "utils");
    }

    #[test]
    fn python_module_name_init() {
        assert_eq!(
            python_module_name(Path::new("src/auth/__init__.py")),
            "auth"
        );
    }

    // ---- Python import analysis ----

    #[test]
    fn analyze_python_imports_with_deps() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();

        std::fs::write(src.join("config.py"), "FOO = 'bar'\n").unwrap();
        std::fs::write(
            src.join("app.py"),
            "from .config import FOO\nprint(FOO)\n",
        )
        .unwrap();

        let files: Vec<&Path> = vec![Path::new("src/config.py"), Path::new("src/app.py")];

        let result = analyze_python_imports(dir.path(), &files).unwrap();
        assert!(result.contains("flowchart TD"));
        assert!(result.contains("app --> config"));
    }

    // ---- Go helpers ----

    #[test]
    fn short_package_name_strips_prefix() {
        assert_eq!(
            short_package_name("github.com/user/repo/pkg/auth", "github.com/user/repo"),
            "pkg/auth"
        );
    }

    #[test]
    fn short_package_name_no_prefix() {
        assert_eq!(short_package_name("fmt", ""), "fmt");
    }

    #[test]
    fn read_go_module_path_basic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module github.com/user/repo\n\ngo 1.21\n",
        )
        .unwrap();

        let result = read_go_module_path(dir.path());
        assert_eq!(result, Some("github.com/user/repo".to_string()));
    }

    #[test]
    fn read_go_module_path_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_go_module_path(dir.path());
        assert!(result.is_none());
    }

    // ---- Plugin trait conformance ----

    #[test]
    fn rust_plugin_name_and_extensions() {
        let p = RustPlugin;
        assert_eq!(p.name(), "rust");
        assert_eq!(p.extensions(), &[".rs"]);
        assert!(p.supported_levels().contains(&DiagramLevel::Container));
        assert!(p.supported_levels().contains(&DiagramLevel::Class));
    }

    #[test]
    fn typescript_plugin_name_and_extensions() {
        let p = TypeScriptPlugin;
        assert_eq!(p.name(), "typescript");
        assert!(p.extensions().contains(&".ts"));
        assert!(p.extensions().contains(&".tsx"));
        assert!(p.extensions().contains(&".js"));
        assert!(p.extensions().contains(&".jsx"));
    }

    #[test]
    fn python_plugin_name_and_extensions() {
        let p = PythonPlugin;
        assert_eq!(p.name(), "python");
        assert!(p.extensions().contains(&".py"));
        assert!(p.extensions().contains(&".pyi"));
    }

    #[test]
    fn go_plugin_name_and_extensions() {
        let p = GoPlugin;
        assert_eq!(p.name(), "go");
        assert_eq!(p.extensions(), &[".go"]);
        assert!(p.supported_levels().contains(&DiagramLevel::Container));
    }
}

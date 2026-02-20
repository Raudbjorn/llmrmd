//! Boundary detection: identify project boundaries within a monorepo.
//!
//! Each boundary is a directory that represents a self-contained project
//! (e.g. a Rust crate, SvelteKit app, or Docker service), detected via
//! sentinel files.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::debug;
use walkdir::WalkDir;

use crate::config::SKIP_DIRS;

/// The type of project boundary detected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryType {
    MonorepoRoot,
    SveltekitApp,
    NextjsApp,
    SupabaseProject,
    RustCrate,
    PythonProject,
    GoModule,
    DockerService,
    NodePackage,
    Generic,
}

impl BoundaryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MonorepoRoot => "monorepo-root",
            Self::SveltekitApp => "sveltekit-app",
            Self::NextjsApp => "nextjs-app",
            Self::SupabaseProject => "supabase-project",
            Self::RustCrate => "rust-crate",
            Self::PythonProject => "python-project",
            Self::GoModule => "go-module",
            Self::DockerService => "docker-service",
            Self::NodePackage => "node-package",
            Self::Generic => "generic",
        }
    }
}

/// A detected project boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryRecord {
    /// The domain name this boundary belongs to.
    pub domain: String,
    /// What type of project boundary this is.
    pub boundary_type: BoundaryType,
    /// Relative path from repo root to the boundary directory.
    pub root_path: String,
    /// Which sentinel files were found.
    pub sentinel_files: Vec<String>,
}

/// Sentinel file pattern for a boundary type.
struct BoundaryPattern {
    boundary_type: BoundaryType,
    /// All of these must be present.
    required: &'static [&'static str],
    /// At least one of these must be present (empty = no requirement).
    any_of: &'static [&'static str],
}

/// Order matters: more specific patterns first so they match before generic ones.
static PATTERNS: &[BoundaryPattern] = &[
    BoundaryPattern {
        boundary_type: BoundaryType::SveltekitApp,
        required: &[],
        any_of: &["svelte.config.js", "svelte.config.ts"],
    },
    BoundaryPattern {
        boundary_type: BoundaryType::NextjsApp,
        required: &[],
        any_of: &["next.config.js", "next.config.mjs", "next.config.ts"],
    },
    BoundaryPattern {
        boundary_type: BoundaryType::SupabaseProject,
        required: &[],
        any_of: &["supabase/config.toml", "config.toml"],
    },
    BoundaryPattern {
        boundary_type: BoundaryType::RustCrate,
        required: &["Cargo.toml"],
        any_of: &[],
    },
    BoundaryPattern {
        boundary_type: BoundaryType::PythonProject,
        required: &[],
        any_of: &["pyproject.toml", "setup.py", "setup.cfg"],
    },
    BoundaryPattern {
        boundary_type: BoundaryType::GoModule,
        required: &["go.mod"],
        any_of: &[],
    },
    BoundaryPattern {
        boundary_type: BoundaryType::DockerService,
        required: &[],
        any_of: &["Dockerfile", "docker-compose.yml", "docker-compose.yaml"],
    },
    BoundaryPattern {
        boundary_type: BoundaryType::NodePackage,
        required: &["package.json"],
        any_of: &[],
    },
];

/// Check if a directory matches a specific boundary pattern.
fn check_pattern(dir: &Path, pattern: &BoundaryPattern) -> Option<Vec<String>> {
    let mut found = Vec::new();

    // All required files must exist
    for &req in pattern.required {
        if dir.join(req).exists() {
            found.push(req.to_string());
        } else {
            return None;
        }
    }

    // At least one of any_of must exist (if any_of is non-empty)
    if !pattern.any_of.is_empty() {
        let mut matched_any = false;
        for &sentinel in pattern.any_of {
            if dir.join(sentinel).exists() {
                found.push(sentinel.to_string());
                matched_any = true;
            }
        }
        if !matched_any {
            return None;
        }
    }

    if found.is_empty() {
        None
    } else {
        Some(found)
    }
}

/// Detect a monorepo root by checking for workspace config files.
fn detect_monorepo_root(dir: &Path) -> Option<Vec<String>> {
    let mut sentinels = Vec::new();

    // Cargo workspace
    if let Ok(content) = std::fs::read_to_string(dir.join("Cargo.toml")) {
        if content.contains("[workspace]") {
            sentinels.push("Cargo.toml".to_string());
        }
    }

    // pnpm workspace
    if dir.join("pnpm-workspace.yaml").exists() {
        sentinels.push("pnpm-workspace.yaml".to_string());
    }

    // npm/yarn workspaces (check package.json for "workspaces" field)
    if let Ok(content) = std::fs::read_to_string(dir.join("package.json")) {
        if content.contains("\"workspaces\"") {
            sentinels.push("package.json".to_string());
        }
    }

    // Lerna
    if dir.join("lerna.json").exists() {
        sentinels.push("lerna.json".to_string());
    }

    // Nx
    if dir.join("nx.json").exists() {
        sentinels.push("nx.json".to_string());
    }

    // Turborepo
    if dir.join("turbo.json").exists() {
        sentinels.push("turbo.json".to_string());
    }

    if sentinels.is_empty() {
        None
    } else {
        Some(sentinels)
    }
}

/// Infer the domain name for a boundary from its path relative to the repo root.
fn infer_boundary_domain(rel_path: &str) -> String {
    if rel_path.is_empty() || rel_path == "." {
        return "root".to_string();
    }

    let parts: Vec<&str> = rel_path.split('/').collect();

    // Container dirs: apps/web → "web"
    if parts.len() >= 2 {
        let top = parts[0];
        if ["apps", "packages", "services", "libs"].contains(&top) {
            return parts[1].to_string();
        }
    }

    // Leaf dirs: supabase → "supabase"
    if let Some(&first) = parts.first() {
        return first.to_string();
    }

    "root".to_string()
}

/// Detect all project boundaries in a repo.
pub fn detect_boundaries(root: &Path) -> Vec<BoundaryRecord> {
    let mut boundaries = Vec::new();
    let mut seen_dirs: HashSet<String> = HashSet::new();

    // Check root for monorepo
    if let Some(sentinels) = detect_monorepo_root(root) {
        boundaries.push(BoundaryRecord {
            domain: "root".to_string(),
            boundary_type: BoundaryType::MonorepoRoot,
            root_path: ".".to_string(),
            sentinel_files: sentinels,
        });
        seen_dirs.insert(".".to_string());
    }

    // Walk directories (shallow: we only need directories, not files)
    for entry in WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .map(|s| !SKIP_DIRS.contains(s))
                .unwrap_or(true)
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() {
            continue;
        }

        let abs_dir = entry.path();
        let rel_path = match abs_dir.strip_prefix(root) {
            Ok(r) => r.to_str().unwrap_or("").replace('\\', "/"),
            Err(_) => continue,
        };

        if seen_dirs.contains(&rel_path) {
            continue;
        }

        // Check each pattern (most specific first)
        for pattern in PATTERNS {
            if let Some(sentinels) = check_pattern(abs_dir, pattern) {
                let domain = infer_boundary_domain(&rel_path);

                debug!(
                    domain = %domain,
                    boundary_type = pattern.boundary_type.as_str(),
                    path = %rel_path,
                    "Detected boundary"
                );

                boundaries.push(BoundaryRecord {
                    domain,
                    boundary_type: pattern.boundary_type.clone(),
                    root_path: if rel_path.is_empty() {
                        ".".to_string()
                    } else {
                        rel_path.clone()
                    },
                    sentinel_files: sentinels,
                });
                seen_dirs.insert(rel_path.clone());
                break; // First match wins (most specific)
            }
        }
    }

    boundaries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rust_crate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::RustCrate));
    }

    #[test]
    fn detect_node_package() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::NodePackage));
    }

    #[test]
    fn detect_sveltekit_app() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("svelte.config.js"), "export default {}").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();

        let boundaries = detect_boundaries(dir.path());
        // SvelteKit should match before NodePackage (more specific)
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::SveltekitApp));
    }

    #[test]
    fn detect_nextjs_app() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("next.config.js"), "module.exports = {}").unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::NextjsApp));
    }

    #[test]
    fn detect_python_project() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"test\"",
        )
        .unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::PythonProject));
    }

    #[test]
    fn detect_go_module() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com/test").unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::GoModule));
    }

    #[test]
    fn detect_docker_service() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Dockerfile"), "FROM alpine").unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::DockerService));
    }

    #[test]
    fn detect_monorepo_root_pnpm() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - apps/*",
        )
        .unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::MonorepoRoot));
    }

    #[test]
    fn detect_monorepo_root_cargo_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]",
        )
        .unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::MonorepoRoot));
    }

    #[test]
    fn detect_nested_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        // Root monorepo
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - apps/*",
        )
        .unwrap();
        // Nested SvelteKit app
        let web = dir.path().join("apps").join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("svelte.config.js"), "export default {}").unwrap();
        std::fs::write(web.join("package.json"), "{}").unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::MonorepoRoot && b.domain == "root"));
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::SveltekitApp && b.domain == "web"));
    }

    #[test]
    fn domain_inference_from_path() {
        assert_eq!(infer_boundary_domain(""), "root");
        assert_eq!(infer_boundary_domain("."), "root");
        assert_eq!(infer_boundary_domain("apps/web"), "web");
        assert_eq!(infer_boundary_domain("packages/shared"), "shared");
        assert_eq!(infer_boundary_domain("supabase"), "supabase");
    }

    #[test]
    fn empty_dir_no_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries.is_empty());
    }

    #[test]
    fn supabase_project_detection() {
        let dir = tempfile::tempdir().unwrap();
        let sb = dir.path().join("supabase");
        std::fs::create_dir_all(&sb).unwrap();
        std::fs::write(sb.join("config.toml"), "[project]\nid = \"test\"").unwrap();

        let boundaries = detect_boundaries(dir.path());
        assert!(boundaries
            .iter()
            .any(|b| b.boundary_type == BoundaryType::SupabaseProject));
    }
}

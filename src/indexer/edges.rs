//! Dependency edge extraction: parse manifest files to find cross-domain deps.
//!
//! Parses `package.json`, `Cargo.toml`, `pyproject.toml`, and `go.mod` to
//! extract dependency names, then cross-references against known domain names
//! from the index to produce inter-domain edges.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::boundary::BoundaryRecord;

/// The kind of dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepType {
    Runtime,
    Dev,
    Peer,
}

impl DepType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Dev => "dev",
            Self::Peer => "peer",
        }
    }
}

/// A dependency edge between two domains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeRecord {
    /// The domain that declares the dependency.
    pub source_domain: String,
    /// The domain that is depended upon.
    pub target_domain: String,
    /// Whether this is a runtime, dev, or peer dependency.
    pub dep_type: DepType,
    /// The package/crate name as declared in the manifest.
    pub dep_name: String,
}

/// Raw dependency parsed from a manifest file.
struct RawDep {
    name: String,
    dep_type: DepType,
}

/// Parse dependencies from a `package.json` file.
fn parse_package_json(path: &Path) -> Vec<RawDep> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Could not read package.json");
            return Vec::new();
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Could not parse package.json");
            return Vec::new();
        }
    };

    let mut deps = Vec::new();

    let sections = [
        ("dependencies", DepType::Runtime),
        ("devDependencies", DepType::Dev),
        ("peerDependencies", DepType::Peer),
    ];

    for (key, dep_type) in sections {
        if let Some(obj) = json.get(key).and_then(|v| v.as_object()) {
            for name in obj.keys() {
                deps.push(RawDep {
                    name: name.clone(),
                    dep_type: dep_type.clone(),
                });
            }
        }
    }

    deps
}

/// Parse dependencies from a `Cargo.toml` file.
fn parse_cargo_toml(path: &Path) -> Vec<RawDep> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Could not read Cargo.toml");
            return Vec::new();
        }
    };

    let mut deps = Vec::new();

    // Simple line-by-line parsing: detect [dependencies], [dev-dependencies], [build-dependencies]
    // and extract crate names from subsequent lines until the next section.
    let mut current_section = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            current_section = match trimmed {
                "[dependencies]" => Some(DepType::Runtime),
                "[dev-dependencies]" => Some(DepType::Dev),
                "[build-dependencies]" => Some(DepType::Dev),
                _ if trimmed.starts_with("[dependencies.") => Some(DepType::Runtime),
                _ if trimmed.starts_with("[dev-dependencies.") => Some(DepType::Dev),
                _ => None,
            };
            continue;
        }

        if let Some(ref dep_type) = current_section {
            // Lines like: `serde = "1"` or `serde = { version = "1", features = [...] }`
            if let Some(name) = trimmed.split('=').next() {
                let name = name.trim();
                if !name.is_empty() && !name.starts_with('#') && !name.contains('[') {
                    deps.push(RawDep {
                        name: name.to_string(),
                        dep_type: dep_type.clone(),
                    });
                }
            }
        }
    }

    deps
}

/// Parse dependencies from a `pyproject.toml` file.
fn parse_pyproject_toml(path: &Path) -> Vec<RawDep> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Could not read pyproject.toml");
            return Vec::new();
        }
    };

    let mut deps = Vec::new();
    let mut in_dependencies = false;
    let mut in_dev_deps = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            in_dependencies = trimmed == "dependencies"
                || trimmed == "[project.dependencies]"
                || trimmed == "[tool.poetry.dependencies]";
            in_dev_deps = trimmed.contains("dev-dependencies")
                || trimmed.contains("dev_dependencies")
                || trimmed == "[project.optional-dependencies.dev]";
            continue;
        }

        if in_dependencies || in_dev_deps {
            let dep_type = if in_dev_deps {
                DepType::Dev
            } else {
                DepType::Runtime
            };

            // Handle array items like: `"requests>=2.28"`
            if trimmed.starts_with('"') || trimmed.starts_with('\'') {
                let name = trimmed
                    .trim_matches(|c: char| c == '"' || c == '\'' || c == ',')
                    .split(['>', '<', '=', '!', '['])
                    .next()
                    .unwrap_or("")
                    .trim();
                if !name.is_empty() {
                    deps.push(RawDep {
                        name: name.to_string(),
                        dep_type,
                    });
                }
            }
            // Handle table items like: `requests = "^2.28"`
            else if let Some(name) = trimmed.split('=').next() {
                let name = name.trim();
                if !name.is_empty()
                    && !name.starts_with('#')
                    && !name.starts_with(']')
                    && !name.contains('[')
                {
                    deps.push(RawDep {
                        name: name.to_string(),
                        dep_type,
                    });
                }
            }
        }
    }

    deps
}

/// Parse dependencies from a `go.mod` file.
fn parse_go_mod(path: &Path) -> Vec<RawDep> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Could not read go.mod");
            return Vec::new();
        }
    };

    let mut deps = Vec::new();
    let mut in_require = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("require (") || trimmed == "require (" {
            in_require = true;
            continue;
        }
        if trimmed == ")" {
            in_require = false;
            continue;
        }

        // Single-line require: `require github.com/foo/bar v1.2.3`
        if trimmed.starts_with("require ") && !trimmed.contains('(') {
            let parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
            if parts.len() >= 2 {
                deps.push(RawDep {
                    name: parts[1].to_string(),
                    dep_type: DepType::Runtime,
                });
            }
            continue;
        }

        if in_require {
            // `\tgithub.com/foo/bar v1.2.3`
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if !parts.is_empty() && !parts[0].starts_with("//") {
                deps.push(RawDep {
                    name: parts[0].to_string(),
                    dep_type: DepType::Runtime,
                });
            }
        }
    }

    deps
}

/// Extract dependency edges by cross-referencing manifest deps against known domain names.
///
/// The `domain_names` set should contain all known domain names from the index.
/// Only deps whose name matches (or is a suffix of) a known domain name produce edges.
pub fn extract_edges(
    root: &Path,
    boundaries: &[BoundaryRecord],
    domain_names: &HashSet<String>,
) -> Vec<EdgeRecord> {
    // Build a lookup: package name → domain name, for workspace packages
    let mut pkg_to_domain: HashMap<String, String> = HashMap::new();
    for domain in domain_names {
        pkg_to_domain.insert(domain.clone(), domain.clone());
    }

    // Also try to read package names from each boundary's manifest
    for boundary in boundaries {
        let dir = root.join(&boundary.root_path);
        // package.json: read "name" field
        if let Ok(content) = std::fs::read_to_string(dir.join("package.json")) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                    pkg_to_domain.insert(name.to_string(), boundary.domain.clone());
                    // Also insert the unscoped name (e.g., @scope/pkg → pkg)
                    if let Some(unscoped) = name.strip_prefix('@').and_then(|s| s.split('/').nth(1))
                    {
                        pkg_to_domain.insert(unscoped.to_string(), boundary.domain.clone());
                    }
                }
            }
        }
        // Cargo.toml: read package name
        if let Ok(content) = std::fs::read_to_string(dir.join("Cargo.toml")) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("name") {
                    if let Some(name) = trimmed.split('=').nth(1) {
                        let name = name.trim().trim_matches('"').trim_matches('\'');
                        if !name.is_empty() {
                            pkg_to_domain.insert(name.to_string(), boundary.domain.clone());
                        }
                    }
                    break;
                }
            }
        }
    }

    let mut edges = Vec::new();

    for boundary in boundaries {
        let dir = root.join(&boundary.root_path);
        let source_domain = &boundary.domain;

        let raw_deps = collect_raw_deps(&dir);

        for dep in raw_deps {
            // Check if this dep name matches a known domain/package
            if let Some(target_domain) = pkg_to_domain.get(&dep.name) {
                if target_domain != source_domain {
                    debug!(
                        source = %source_domain,
                        target = %target_domain,
                        dep = %dep.name,
                        "Found cross-domain edge"
                    );
                    edges.push(EdgeRecord {
                        source_domain: source_domain.clone(),
                        target_domain: target_domain.clone(),
                        dep_type: dep.dep_type,
                        dep_name: dep.name,
                    });
                }
            }
        }
    }

    // Deduplicate: same source→target→dep_name should only appear once
    edges.sort_by(|a, b| {
        a.source_domain
            .cmp(&b.source_domain)
            .then(a.target_domain.cmp(&b.target_domain))
            .then(a.dep_name.cmp(&b.dep_name))
    });
    edges.dedup_by(|a, b| {
        a.source_domain == b.source_domain
            && a.target_domain == b.target_domain
            && a.dep_name == b.dep_name
    });

    edges
}

/// Collect all raw dependencies from manifest files in a directory.
fn collect_raw_deps(dir: &Path) -> Vec<RawDep> {
    let mut deps = Vec::new();

    let pkg_json = dir.join("package.json");
    if pkg_json.exists() {
        deps.extend(parse_package_json(&pkg_json));
    }

    let cargo_toml = dir.join("Cargo.toml");
    if cargo_toml.exists() {
        deps.extend(parse_cargo_toml(&cargo_toml));
    }

    let pyproject = dir.join("pyproject.toml");
    if pyproject.exists() {
        deps.extend(parse_pyproject_toml(&pyproject));
    }

    let go_mod = dir.join("go.mod");
    if go_mod.exists() {
        deps.extend(parse_go_mod(&go_mod));
    }

    deps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::boundary::BoundaryType;

    #[test]
    fn parse_package_json_deps() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("package.json");
        std::fs::write(
            &pkg,
            r#"{
                "dependencies": { "react": "^18", "shared": "workspace:*" },
                "devDependencies": { "vitest": "^1" },
                "peerDependencies": { "react-dom": "^18" }
            }"#,
        )
        .unwrap();

        let deps = parse_package_json(&pkg);
        assert_eq!(deps.len(), 4);
        assert!(deps
            .iter()
            .any(|d| d.name == "react" && d.dep_type == DepType::Runtime));
        assert!(deps
            .iter()
            .any(|d| d.name == "shared" && d.dep_type == DepType::Runtime));
        assert!(deps
            .iter()
            .any(|d| d.name == "vitest" && d.dep_type == DepType::Dev));
        assert!(deps
            .iter()
            .any(|d| d.name == "react-dom" && d.dep_type == DepType::Peer));
    }

    #[test]
    fn parse_cargo_toml_deps() {
        let dir = tempfile::tempdir().unwrap();
        let cargo = dir.path().join("Cargo.toml");
        std::fs::write(
            &cargo,
            r#"[package]
name = "test"

[dependencies]
serde = "1"
shared-lib = { path = "../shared" }

[dev-dependencies]
tempfile = "3"
"#,
        )
        .unwrap();

        let deps = parse_cargo_toml(&cargo);
        assert!(deps
            .iter()
            .any(|d| d.name == "serde" && d.dep_type == DepType::Runtime));
        assert!(deps
            .iter()
            .any(|d| d.name == "shared-lib" && d.dep_type == DepType::Runtime));
        assert!(deps
            .iter()
            .any(|d| d.name == "tempfile" && d.dep_type == DepType::Dev));
    }

    #[test]
    fn parse_go_mod_deps() {
        let dir = tempfile::tempdir().unwrap();
        let gomod = dir.path().join("go.mod");
        std::fs::write(
            &gomod,
            "module example.com/myapp\n\nrequire (\n\tgithub.com/gin-gonic/gin v1.9.1\n\tgithub.com/lib/pq v1.10.9\n)\n",
        )
        .unwrap();

        let deps = parse_go_mod(&gomod);
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().any(|d| d.name == "github.com/gin-gonic/gin"));
    }

    #[test]
    fn extract_edges_cross_domain() {
        let dir = tempfile::tempdir().unwrap();
        // Create web app that depends on "shared"
        let web = dir.path().join("apps").join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(
            web.join("package.json"),
            r#"{ "name": "@my/web", "dependencies": { "@my/shared": "workspace:*" } }"#,
        )
        .unwrap();

        // Create shared package
        let shared = dir.path().join("packages").join("shared");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("package.json"), r#"{ "name": "@my/shared" }"#).unwrap();

        let boundaries = vec![
            BoundaryRecord {
                domain: "web".to_string(),
                boundary_type: BoundaryType::NodePackage,
                root_path: "apps/web".to_string(),
                sentinel_files: vec!["package.json".to_string()],
            },
            BoundaryRecord {
                domain: "shared".to_string(),
                boundary_type: BoundaryType::NodePackage,
                root_path: "packages/shared".to_string(),
                sentinel_files: vec!["package.json".to_string()],
            },
        ];

        let domains: HashSet<String> = ["web", "shared"].iter().map(|s| s.to_string()).collect();
        let edges = extract_edges(dir.path(), &boundaries, &domains);

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source_domain, "web");
        assert_eq!(edges[0].target_domain, "shared");
    }

    #[test]
    fn no_self_edges() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{ "name": "root", "dependencies": { "root": "1.0" } }"#,
        )
        .unwrap();

        let boundaries = vec![BoundaryRecord {
            domain: "root".to_string(),
            boundary_type: BoundaryType::NodePackage,
            root_path: ".".to_string(),
            sentinel_files: vec!["package.json".to_string()],
        }];

        let domains: HashSet<String> = ["root"].iter().map(|s| s.to_string()).collect();
        let edges = extract_edges(dir.path(), &boundaries, &domains);
        assert!(
            edges.is_empty(),
            "Should not produce self-referencing edges"
        );
    }

    #[test]
    fn external_deps_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{ "name": "test", "dependencies": { "react": "^18", "express": "^4" } }"#,
        )
        .unwrap();

        let boundaries = vec![BoundaryRecord {
            domain: "root".to_string(),
            boundary_type: BoundaryType::NodePackage,
            root_path: ".".to_string(),
            sentinel_files: vec!["package.json".to_string()],
        }];

        // No domain named "react" or "express"
        let domains: HashSet<String> = ["root"].iter().map(|s| s.to_string()).collect();
        let edges = extract_edges(dir.path(), &boundaries, &domains);
        assert!(edges.is_empty(), "External deps should not produce edges");
    }
}

//! Indexer: Scans a monorepo and produces structured artifacts for LLM context.
//!
//! Outputs to `.claude/`:
//! - `file-index.toon` / `.json` — compact file listing
//! - `manifest.toon` / `.json` — diagram inventory
//! - `diagrams/extracted/*.mmd` — extracted mermaid diagrams

pub mod mermaid;
pub mod toon;
pub mod types;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::Utc;
use regex::Regex;
use tracing::{info, warn};
use walkdir::WalkDir;

use crate::config::*;
use crate::error::{self, Error, Result};
use types::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn should_skip_file(rel_path: &Path) -> bool {
    // Check directory components
    for component in rel_path.parent().iter().flat_map(|p| p.components()) {
        if let Some(s) = component.as_os_str().to_str() {
            if SKIP_DIRS.contains(s) {
                return true;
            }
        }
    }

    let name = rel_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    if SKIP_FILE_EXACT.contains(name) {
        return true;
    }

    let ext = rel_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_default();
    if SKIP_FILE_SUFFIXES.contains(ext.as_str()) {
        return true;
    }

    // Hidden files (but not .env variants)
    if name.starts_with('.') && !name.starts_with(".env") {
        return true;
    }

    false
}

fn infer_type(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    if let Some(&t) = FILENAME_TYPE_MAP.get(name) {
        return t;
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()))
        .unwrap_or_default();

    EXTENSION_TYPE_MAP.get(ext.as_str()).copied().unwrap_or("file")
}

fn infer_domain(rel_path: &Path) -> String {
    let components: Vec<&str> = rel_path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    if components.len() <= 1 {
        return "root".to_string();
    }

    let top = components[0];

    // Container dirs: apps/web → "web"
    if DOMAIN_CONTAINER_DIRS.contains(top) && components.len() > 2 {
        return components[1].to_string();
    }

    // Leaf dirs: supabase/anything → "supabase"
    if DOMAIN_LEAF_DIRS.contains(top) {
        return top.to_string();
    }

    top.to_string()
}

/// Infer semantic subdomain from path (v6 heuristic).
fn infer_subdomain(rel_posix: &str) -> &'static str {
    let path_with_slash = format!("/{rel_posix}");
    for (pattern_str, layer) in SUBDOMAIN_RULES.iter() {
        // We use a simple contains check first for performance,
        // falling back to regex only if needed.
        if let Ok(re) = Regex::new(pattern_str) {
            if re.is_match(&path_with_slash) {
                return layer;
            }
        }
    }
    "core"
}

/// Find all directories containing a CLAUDE.md.
fn find_claude_md_dirs(root: &Path) -> HashSet<PathBuf> {
    let mut dirs = HashSet::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .map(|s| !SKIP_DIRS.contains(s))
                .unwrap_or(true)
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() && entry.file_name() == "CLAUDE.md" {
            if let Some(parent) = entry.path().parent() {
                dirs.insert(parent.to_path_buf());
            }
        }
    }

    dirs
}

/// Find the nearest CLAUDE.md for a file, walking up to root.
fn nearest_claude_md(file_path: &Path, claude_md_dirs: &HashSet<PathBuf>, root: &Path) -> String {
    let mut current = file_path.parent().unwrap_or(root).to_path_buf();

    loop {
        if claude_md_dirs.contains(&current) {
            let claude_path = current.join("CLAUDE.md");
            return claude_path
                .strip_prefix(root)
                .ok()
                .and_then(|p| p.to_str())
                .map(|s| s.replace('\\', "/"))
                .unwrap_or_default();
        }

        if current == root || current.parent().is_none() {
            if claude_md_dirs.contains(&root.to_path_buf()) {
                return "CLAUDE.md".to_string();
            }
            return String::new();
        }

        current = current.parent().unwrap().to_path_buf();
    }
}

// ---------------------------------------------------------------------------
// Core scan logic
// ---------------------------------------------------------------------------

/// Walk the repository and collect file records and mermaid diagrams.
pub fn scan_repo(root: &Path) -> Result<IndexResult> {
    let root = root
        .canonicalize()
        .map_err(|e| error::io_err(root, e))?;

    if !root.is_dir() {
        return Err(Error::InvalidRoot(root));
    }

    info!(root = %root.display(), "Scanning repo");

    let claude_md_dirs = find_claude_md_dirs(&root);
    let mut claude_md_paths: Vec<String> = claude_md_dirs
        .iter()
        .filter_map(|d| {
            let p = d.join("CLAUDE.md");
            p.strip_prefix(&root)
                .ok()
                .and_then(|r| r.to_str())
                .map(|s| s.replace('\\', "/"))
        })
        .collect();
    claude_md_paths.sort();

    info!(count = claude_md_paths.len(), "Found CLAUDE.md files");

    let mut files: Vec<FileRecord> = Vec::new();
    let mut diagrams: Vec<DiagramRecord> = Vec::new();

    for entry in WalkDir::new(&root)
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
        if !entry.file_type().is_file() {
            continue;
        }

        let abs_path = entry.path();
        let rel_path = match abs_path.strip_prefix(&root) {
            Ok(r) => r,
            Err(_) => continue,
        };

        if should_skip_file(rel_path) {
            continue;
        }

        let posix_path = rel_path.to_str().unwrap_or("").replace('\\', "/");
        let domain = infer_domain(rel_path);
        let subdomain = infer_subdomain(&posix_path);

        files.push(FileRecord {
            path: posix_path.clone(),
            file_type: infer_type(abs_path).to_string(),
            domain: domain.clone(),
            subdomain: subdomain.to_string(),
            claude_md: nearest_claude_md(abs_path, &claude_md_dirs, &root),
        });

        // Mermaid extraction
        let raw_diagrams = mermaid::extract_from_file(abs_path);
        let diag_count = raw_diagrams.len();
        for (i, raw) in raw_diagrams.into_iter().enumerate() {
            let minified = mermaid::minify(&raw.content);
            if minified.is_empty() {
                continue;
            }

            let source = if diag_count > 1 {
                format!("{}#diag-{}", posix_path, i + 1)
            } else {
                posix_path.clone()
            };

            let id = raw.id.unwrap_or_else(|| {
                // Auto-generate ID from source path
                source
                    .replace(['/', '#', '.'], "-")
                    .trim_matches('-')
                    .to_string()
            });

            diagrams.push(DiagramRecord {
                id,
                source,
                domain: domain.clone(),
                diagram_type: mermaid::infer_type(&minified).to_string(),
                tokens_est: mermaid::estimate_tokens(&minified),
                content: minified,
            });
        }
    }

    info!(
        files = files.len(),
        diagrams = diagrams.len(),
        "Scan complete"
    );

    let root_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(IndexResult {
        root: root.clone(),
        generated_at: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        root_name,
        files,
        diagrams,
        claude_md_paths,
    })
}

/// Write all index artifacts to `.claude/`.
pub fn write_index(result: &IndexResult, dry_run: bool) -> Result<()> {
    let out = output_dir(&result.root);

    if dry_run {
        let toon_str = render_full_toon(result);
        let manifest_str = render_manifest_toon(result);
        print!("{toon_str}\n\n{manifest_str}");
        info!(
            files = result.files.len(),
            diagrams = result.diagrams.len(),
            "Dry run complete"
        );
        return Ok(());
    }

    std::fs::create_dir_all(&out).map_err(|e| error::io_err(&out, e))?;

    // file-index.toon
    let toon_str = render_full_toon(result);
    let toon_path = out.join("file-index.toon");
    std::fs::write(&toon_path, &toon_str).map_err(|e| error::io_err(&toon_path, e))?;
    info!(path = %toon_path.display(), bytes = toon_str.len(), "Wrote file-index.toon");

    // file-index.json
    let json_data = serde_json::json!({
        "generated_at": result.generated_at,
        "root": result.root_name,
        "claude_md_paths": result.claude_md_paths,
        "files": result.files,
    });
    let json_path = out.join("file-index.json");
    let json_str = serde_json::to_string_pretty(&json_data)
        .map_err(|e| Error::Json { path: json_path.clone(), source: e })?;
    std::fs::write(&json_path, &json_str).map_err(|e| error::io_err(&json_path, e))?;
    info!(path = %json_path.display(), "Wrote file-index.json");

    // manifest.toon
    let manifest_str = render_manifest_toon(result);
    let manifest_path = out.join("manifest.toon");
    std::fs::write(&manifest_path, &manifest_str).map_err(|e| error::io_err(&manifest_path, e))?;
    info!(path = %manifest_path.display(), "Wrote manifest.toon");

    // manifest.json
    let manifest_json = serde_json::json!({
        "generated_at": result.generated_at,
        "diagrams": result.diagrams.iter().map(|d| serde_json::json!({
            "id": d.id,
            "source": d.source,
            "domain": d.domain,
            "type": d.diagram_type,
            "tokens_est": d.tokens_est,
        })).collect::<Vec<_>>(),
    });
    let mj_path = out.join("manifest.json");
    let mj_str = serde_json::to_string_pretty(&manifest_json)
        .map_err(|e| Error::Json { path: mj_path.clone(), source: e })?;
    std::fs::write(&mj_path, &mj_str).map_err(|e| error::io_err(&mj_path, e))?;
    info!(path = %mj_path.display(), "Wrote manifest.json");

    // diagrams/extracted/*.mmd
    if !result.diagrams.is_empty() {
        let diag_dir = out.join("diagrams").join("extracted");
        std::fs::create_dir_all(&diag_dir).map_err(|e| error::io_err(&diag_dir, e))?;

        for diag in &result.diagrams {
            let safe_name = format!(
                "{}.mmd",
                diag.source
                    .replace('/', "__")
                    .replace('#', "_")
                    .trim_end_matches(".mmd")
            );
            let diag_path = diag_dir.join(&safe_name);
            if let Err(e) = std::fs::write(&diag_path, &diag.content) {
                warn!(path = %diag_path.display(), error = %e, "Failed to write diagram");
            }
        }

        info!(count = result.diagrams.len(), dir = %diag_dir.display(), "Wrote extracted diagrams");
    }

    info!(
        files = result.files.len(),
        diagrams = result.diagrams.len(),
        output = %out.display(),
        "Indexing complete"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// TOON rendering
// ---------------------------------------------------------------------------

fn render_full_toon(result: &IndexResult) -> String {
    let mut sections = Vec::new();

    sections.push(format!(
        "# REPO FILE INDEX — generated {}\n# Root: {}\n# Files: {}  Diagrams: {}",
        result.generated_at,
        result.root_name,
        result.files.len(),
        result.diagrams.len(),
    ));

    if !result.claude_md_paths.is_empty() {
        let rows: Vec<Vec<&str>> = result.claude_md_paths.iter().map(|p| vec![p.as_str()]).collect();
        sections.push(toon::render_tabular(
            "claude_docs",
            &["path"],
            &rows,
            Some("CLAUDE.md locations (progressive disclosure chain)"),
        ));
    }

    let file_rows: Vec<Vec<&str>> = result
        .files
        .iter()
        .map(|f| {
            vec![
                f.path.as_str(),
                f.file_type.as_str(),
                f.domain.as_str(),
                f.subdomain.as_str(),
                f.claude_md.as_str(),
            ]
        })
        .collect();

    sections.push(toon::render_tabular(
        "files",
        &["path", "type", "domain", "subdomain", "claude_md"],
        &file_rows,
        Some("FILE INDEX"),
    ));

    sections.join("\n\n") + "\n"
}

fn render_manifest_toon(result: &IndexResult) -> String {
    let mut sections = Vec::new();

    sections.push(format!(
        "# DIAGRAM MANIFEST — generated {}\n# Use this to select which diagrams to load for planning.",
        result.generated_at,
    ));

    if result.diagrams.is_empty() {
        sections.push("# No diagrams found.".to_string());
        return sections.join("\n") + "\n";
    }

    let tokens_strs: Vec<String> = result.diagrams.iter().map(|d| d.tokens_est.to_string()).collect();
    let rows: Vec<Vec<&str>> = result
        .diagrams
        .iter()
        .zip(tokens_strs.iter())
        .map(|(d, t)| {
            vec![
                d.id.as_str(),
                d.source.as_str(),
                d.domain.as_str(),
                d.diagram_type.as_str(),
                t.as_str(),
            ]
        })
        .collect();

    sections.push(toon::render_tabular(
        "diagrams",
        &["id", "source", "domain", "type", "tokens"],
        &rows,
        Some("AVAILABLE DIAGRAMS (load selectively based on task scope)"),
    ));

    sections.join("\n\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn should_skip_node_modules() {
        assert!(should_skip_file(Path::new("node_modules/express/index.js")));
    }

    #[test]
    fn should_skip_git_dir() {
        assert!(should_skip_file(Path::new(".git/config")));
    }

    #[test]
    fn should_skip_lock_files() {
        assert!(should_skip_file(Path::new("package-lock.json")));
        assert!(should_skip_file(Path::new("pnpm-lock.yaml")));
    }

    #[test]
    fn should_skip_map_suffix() {
        assert!(should_skip_file(Path::new("bundle.js.map")));
    }

    #[test]
    fn should_skip_hidden_files() {
        assert!(should_skip_file(Path::new(".hidden")));
        assert!(should_skip_file(Path::new(".gitattributes")));
    }

    #[test]
    fn should_not_skip_env_files() {
        assert!(!should_skip_file(Path::new(".env")));
        assert!(!should_skip_file(Path::new(".env.local")));
    }

    #[test]
    fn should_not_skip_normal_files() {
        assert!(!should_skip_file(Path::new("src/main.rs")));
        assert!(!should_skip_file(Path::new("apps/web/index.ts")));
    }

    #[test]
    fn infer_type_by_filename() {
        assert_eq!(infer_type(Path::new("CLAUDE.md")), "claude_md");
        assert_eq!(infer_type(Path::new("README.md")), "docs");
        assert_eq!(infer_type(Path::new("Dockerfile")), "container");
    }

    #[test]
    fn infer_type_by_extension() {
        assert_eq!(infer_type(Path::new("App.svelte")), "component");
        assert_eq!(infer_type(Path::new("main.rs")), "module");
        assert_eq!(infer_type(Path::new("schema.sql")), "migration");
        assert_eq!(infer_type(Path::new("style.css")), "style");
    }

    #[test]
    fn infer_type_fallback() {
        assert_eq!(infer_type(Path::new("unknown.xyz")), "file");
    }

    #[test]
    fn infer_domain_container() {
        assert_eq!(infer_domain(Path::new("apps/web/src/main.ts")), "web");
        assert_eq!(infer_domain(Path::new("apps/admin/index.ts")), "admin");
        assert_eq!(infer_domain(Path::new("packages/shared/index.ts")), "shared");
    }

    #[test]
    fn infer_domain_leaf() {
        assert_eq!(infer_domain(Path::new("supabase/migrations/001.sql")), "supabase");
        assert_eq!(infer_domain(Path::new("infra/terraform/main.tf")), "infra");
    }

    #[test]
    fn infer_domain_root() {
        assert_eq!(infer_domain(Path::new("Cargo.toml")), "root");
    }

    #[test]
    fn infer_domain_shallow_container() {
        assert_eq!(infer_domain(Path::new("apps/file.ts")), "apps");
    }

    #[test]
    fn infer_subdomain_routes() {
        assert_eq!(infer_subdomain("apps/web/src/routes/+page.svelte"), "routes");
        assert_eq!(infer_subdomain("src/pages/index.tsx"), "routes");
    }

    #[test]
    fn infer_subdomain_components() {
        assert_eq!(infer_subdomain("src/components/Button.svelte"), "components");
        assert_eq!(infer_subdomain("src/atoms/Icon.svelte"), "components");
    }

    #[test]
    fn infer_subdomain_services() {
        assert_eq!(infer_subdomain("src/services/auth.ts"), "services");
    }

    #[test]
    fn infer_subdomain_state() {
        assert_eq!(infer_subdomain("src/stores/user.ts"), "state");
    }

    #[test]
    fn infer_subdomain_utils() {
        assert_eq!(infer_subdomain("src/utils/format.ts"), "utils");
    }

    #[test]
    fn infer_subdomain_core_fallback() {
        assert_eq!(infer_subdomain("src/main.ts"), "core");
    }

    #[test]
    fn scan_repo_on_temp_dir() {
        let dir = tempfile::tempdir().unwrap();

        let apps_web = dir.path().join("apps").join("web").join("src");
        std::fs::create_dir_all(&apps_web).unwrap();
        std::fs::write(apps_web.join("index.ts"), "export default {}").unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "# Root conventions").unwrap();

        let mmd_dir = dir.path().join("docs");
        std::fs::create_dir_all(&mmd_dir).unwrap();
        std::fs::write(mmd_dir.join("arch.mmd"), "flowchart LR\n  A-->B").unwrap();

        let result = scan_repo(dir.path()).unwrap();

        assert!(!result.files.is_empty());
        assert!(result.files.iter().any(|f| f.path.contains("index.ts")));
        assert!(result.files.iter().any(|f| f.file_type == "claude_md"));
        assert!(!result.diagrams.is_empty());
        assert_eq!(result.diagrams[0].diagram_type, "flowchart");
    }

    #[test]
    fn scan_repo_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = scan_repo(dir.path()).unwrap();
        assert!(result.files.is_empty());
        assert!(result.diagrams.is_empty());
    }

    #[test]
    fn write_index_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.ts"), "export {}").unwrap();

        let result = scan_repo(dir.path()).unwrap();
        write_index(&result, true).unwrap();
        assert!(!dir.path().join(".claude").exists());
    }

    #[test]
    fn write_index_creates_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.ts"), "export {}").unwrap();

        let result = scan_repo(dir.path()).unwrap();
        write_index(&result, false).unwrap();

        let claude_dir = dir.path().join(".claude");
        assert!(claude_dir.join("file-index.toon").exists());
        assert!(claude_dir.join("file-index.json").exists());
        assert!(claude_dir.join("manifest.toon").exists());
        assert!(claude_dir.join("manifest.json").exists());
    }

    #[test]
    fn write_index_with_diagrams() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("arch.mmd"), "flowchart LR\n  A-->B").unwrap();

        let result = scan_repo(dir.path()).unwrap();
        assert!(!result.diagrams.is_empty());

        write_index(&result, false).unwrap();

        let extracted_dir = dir.path().join(".claude").join("diagrams").join("extracted");
        assert!(extracted_dir.exists());
        let entries: Vec<_> = std::fs::read_dir(&extracted_dir).unwrap().collect();
        assert!(!entries.is_empty());
    }

    #[test]
    fn render_full_toon_format() {
        let result = IndexResult {
            root: PathBuf::from("/tmp/test"),
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            root_name: "test".to_string(),
            files: vec![FileRecord {
                path: "src/main.rs".to_string(),
                file_type: "module".to_string(),
                domain: "root".to_string(),
                subdomain: "core".to_string(),
                claude_md: String::new(),
            }],
            diagrams: Vec::new(),
            claude_md_paths: Vec::new(),
        };

        let toon = render_full_toon(&result);
        assert!(toon.contains("REPO FILE INDEX"));
        assert!(toon.contains("files[1]{path,type,domain,subdomain,claude_md}:"));
        assert!(toon.contains("src/main.rs,module,root,core"));
    }
}

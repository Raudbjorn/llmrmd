//! Configuration constants for llmermaid.
//!
//! Ported from the Python proof-of-concept with additions from v6 "Context Compiler".
//! All constants are compile-time known; no runtime config loading needed.

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Directory / file filtering
// ---------------------------------------------------------------------------

/// Directories to always skip during traversal.
pub static SKIP_DIRS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "node_modules",
        ".git",
        ".svelte-kit",
        "dist",
        "build",
        "target",
        ".turbo",
        ".next",
        "__pycache__",
        ".venv",
        "venv",
        ".cache",
        "coverage",
        ".nuxt",
        ".output",
        ".parcel-cache",
    ]
    .into_iter()
    .collect()
});

/// File names to skip entirely.
pub static SKIP_FILE_EXACT: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        ".DS_Store",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "Thumbs.db",
    ]
    .into_iter()
    .collect()
});

/// File suffixes to skip.
pub static SKIP_FILE_SUFFIXES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [".map", ".lock", ".pyc", ".o", ".bin", ".exe", ".wasm"]
        .into_iter()
        .collect()
});

/// "Noise" files that shouldn't count toward extension-majority detection.
pub static NOISE_FILES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        ".keep",
        ".gitkeep",
        "readme.md",
        ".gitignore",
        ".ds_store",
        "license",
        "license.md",
    ]
    .into_iter()
    .collect()
});

// ---------------------------------------------------------------------------
// File type inference
// ---------------------------------------------------------------------------

/// Map file extension → semantic type.
pub static EXTENSION_TYPE_MAP: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        (".svelte", "component"),
        (".tsx", "component"),
        (".jsx", "component"),
        (".vue", "component"),
        (".ts", "module"),
        (".js", "module"),
        (".py", "module"),
        (".rs", "module"),
        (".go", "module"),
        (".scala", "module"),
        (".java", "module"),
        (".css", "style"),
        (".scss", "style"),
        (".html", "template"),
        (".json", "config"),
        (".yaml", "config"),
        (".yml", "config"),
        (".toml", "config"),
        (".sql", "migration"),
        (".md", "docs"),
        (".mermaid", "diagram"),
        (".mmd", "diagram"),
        (".tf", "infra"),
        (".hcl", "infra"),
        (".sh", "script"),
        (".bash", "script"),
        (".zsh", "script"),
        (".fish", "script"),
        (".dockerfile", "container"),
    ]
    .into_iter()
    .collect()
});

/// Map specific filenames → semantic type (higher priority than extension).
pub static FILENAME_TYPE_MAP: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        ("CLAUDE.md", "claude_md"),
        ("README.md", "docs"),
        ("CHANGELOG.md", "docs"),
        ("Dockerfile", "container"),
        ("docker-compose.yml", "container"),
        ("docker-compose.yaml", "container"),
        (".env", "env"),
        (".env.local", "env"),
        (".env.example", "env"),
        ("Makefile", "build"),
        ("Justfile", "build"),
        ("Taskfile.yml", "build"),
    ]
    .into_iter()
    .collect()
});

// ---------------------------------------------------------------------------
// Mermaid extraction
// ---------------------------------------------------------------------------

/// Extensions that are pure mermaid files (entire content is diagram).
pub static MERMAID_FILE_EXTENSIONS: Lazy<HashSet<&'static str>> =
    Lazy::new(|| [".mmd", ".mermaid"].into_iter().collect());

/// Extensions to scan for embedded ```mermaid fenced blocks.
pub static MERMAID_SOURCE_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [".md", ".mdx", ".svelte", ".tsx", ".jsx", ".vue", ".html"]
        .into_iter()
        .collect()
});

/// Max file size to scan for mermaid blocks (256 KiB).
pub const MAX_MERMAID_SCAN_BYTES: u64 = 256 * 1024;

/// Rough token estimation: 1 token ≈ 4 chars for English/code.
pub const CHARS_PER_TOKEN: usize = 4;

// ---------------------------------------------------------------------------
// Domain inference
// ---------------------------------------------------------------------------

/// Container dirs: the *next* level down is the domain name.
/// e.g. `apps/web/...` → domain = "web"
pub static DOMAIN_CONTAINER_DIRS: Lazy<HashSet<&'static str>> =
    Lazy::new(|| ["apps", "packages", "services", "libs"].into_iter().collect());

/// Leaf dirs: the dir itself IS the domain.
/// e.g. `supabase/migrations/...` → domain = "supabase"
pub static DOMAIN_LEAF_DIRS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["supabase", "infra", "terraform", "scripts", "docs", "deploy"]
        .into_iter()
        .collect()
});

/// Structural prefixes to strip when building domain labels (v6 approach).
pub static STRUCTURAL_PREFIXES: Lazy<HashSet<&'static str>> =
    Lazy::new(|| ["apps", "packages", "src", "lib"].into_iter().collect());

// ---------------------------------------------------------------------------
// Semantic subdomain rules (v6 "Context Compiler")
// ---------------------------------------------------------------------------

/// Regex patterns mapping path segments to architectural layers.
pub static SUBDOMAIN_RULES: Lazy<Vec<(&'static str, &'static str)>> = Lazy::new(|| {
    vec![
        (r"/(?:routes|app|pages|api|controllers)(?:/|$)", "routes"),
        (
            r"/(?:components|ui|views|widgets|stories|atoms|molecules|organisms|templates)(?:/|$)",
            "components",
        ),
        (
            r"/(?:services|actions|use-cases|entities)(?:/|$)",
            "services",
        ),
        (
            r"/(?:repositories|models|db|dal|cms|migrations)(?:/|$)",
            "repositories",
        ),
        (
            r"/(?:stores|state|contexts|realtime|reducers)(?:/|$)",
            "state",
        ),
        (
            r"/(?:clients|integrations|external)(?:/|$)",
            "clients",
        ),
        (
            r"/(?:utils|constants|helpers|data|params|types)(?:/|$)",
            "utils",
        ),
    ]
});

// ---------------------------------------------------------------------------
// Output paths
// ---------------------------------------------------------------------------

/// Get the output directory for index artifacts: `<root>/.claude/`
pub fn output_dir(root: &Path) -> PathBuf {
    root.join(".claude")
}

/// Default soft token budget for planning context.
pub const DEFAULT_TOKEN_BUDGET: usize = 4000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_dirs_contains_expected() {
        assert!(SKIP_DIRS.contains("node_modules"));
        assert!(SKIP_DIRS.contains(".git"));
        assert!(SKIP_DIRS.contains("target"));
        assert!(!SKIP_DIRS.contains("src"));
    }

    #[test]
    fn skip_file_exact_contains_expected() {
        assert!(SKIP_FILE_EXACT.contains(".DS_Store"));
        assert!(SKIP_FILE_EXACT.contains("pnpm-lock.yaml"));
        assert!(!SKIP_FILE_EXACT.contains("Cargo.toml"));
    }

    #[test]
    fn skip_file_suffixes_contains_expected() {
        assert!(SKIP_FILE_SUFFIXES.contains(".map"));
        assert!(SKIP_FILE_SUFFIXES.contains(".wasm"));
        assert!(!SKIP_FILE_SUFFIXES.contains(".rs"));
    }

    #[test]
    fn extension_type_map_coverage() {
        assert_eq!(EXTENSION_TYPE_MAP.get(".svelte"), Some(&"component"));
        assert_eq!(EXTENSION_TYPE_MAP.get(".rs"), Some(&"module"));
        assert_eq!(EXTENSION_TYPE_MAP.get(".sql"), Some(&"migration"));
        assert_eq!(EXTENSION_TYPE_MAP.get(".mmd"), Some(&"diagram"));
        assert_eq!(EXTENSION_TYPE_MAP.get(".unknown"), None);
    }

    #[test]
    fn filename_type_map_priority() {
        assert_eq!(FILENAME_TYPE_MAP.get("CLAUDE.md"), Some(&"claude_md"));
        assert_eq!(FILENAME_TYPE_MAP.get("Dockerfile"), Some(&"container"));
        assert_eq!(FILENAME_TYPE_MAP.get(".env"), Some(&"env"));
    }

    #[test]
    fn mermaid_extensions() {
        assert!(MERMAID_FILE_EXTENSIONS.contains(".mmd"));
        assert!(MERMAID_FILE_EXTENSIONS.contains(".mermaid"));
        assert!(!MERMAID_FILE_EXTENSIONS.contains(".md"));
    }

    #[test]
    fn mermaid_source_extensions() {
        assert!(MERMAID_SOURCE_EXTENSIONS.contains(".md"));
        assert!(MERMAID_SOURCE_EXTENSIONS.contains(".svelte"));
        assert!(!MERMAID_SOURCE_EXTENSIONS.contains(".rs"));
    }

    #[test]
    fn domain_container_dirs() {
        assert!(DOMAIN_CONTAINER_DIRS.contains("apps"));
        assert!(DOMAIN_CONTAINER_DIRS.contains("packages"));
        assert!(!DOMAIN_CONTAINER_DIRS.contains("supabase"));
    }

    #[test]
    fn domain_leaf_dirs() {
        assert!(DOMAIN_LEAF_DIRS.contains("supabase"));
        assert!(DOMAIN_LEAF_DIRS.contains("infra"));
        assert!(!DOMAIN_LEAF_DIRS.contains("apps"));
    }

    #[test]
    fn output_dir_path() {
        let root = Path::new("/tmp/repo");
        assert_eq!(output_dir(root), PathBuf::from("/tmp/repo/.claude"));
    }

    #[test]
    fn constants_values() {
        assert_eq!(MAX_MERMAID_SCAN_BYTES, 256 * 1024);
        assert_eq!(CHARS_PER_TOKEN, 4);
        assert_eq!(DEFAULT_TOKEN_BUDGET, 4000);
    }
}

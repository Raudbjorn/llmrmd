//! Scope inference: map a change description to relevant domains.

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use tracing::info;

use super::types::ManifestEntry;
use crate::indexer::types::FileRecord;

/// Keyword → candidate domain mappings.
static KEYWORD_MAP: Lazy<HashMap<&'static str, Vec<&'static str>>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("database", vec!["supabase", "migrations", "db", "prisma"]);
    m.insert("schema", vec!["supabase", "migrations", "db", "prisma"]);
    m.insert("migration", vec!["supabase", "migrations"]);
    m.insert("table", vec!["supabase", "migrations"]);
    m.insert("column", vec!["supabase", "migrations"]);
    m.insert("rls", vec!["supabase"]);
    m.insert("postgis", vec!["supabase"]);
    m.insert("sql", vec!["supabase"]);
    m.insert("api", vec!["admin", "web", "api"]);
    m.insert("endpoint", vec!["admin", "web", "api"]);
    m.insert("route", vec!["admin", "web"]);
    m.insert("crud", vec!["admin"]);
    m.insert("dashboard", vec!["admin"]);
    m.insert("page", vec!["admin", "web"]);
    m.insert("auth", vec!["auth", "shared"]);
    m.insert("frontend", vec!["web"]);
    m.insert("admin", vec!["admin"]);
    m.insert("ui", vec!["web", "admin"]);
    m.insert("shared", vec!["shared"]);
    m.insert("type", vec!["shared", "types"]);
    m.insert("types", vec!["shared"]);
    m.insert("interface", vec!["shared"]);
    m.insert("validator", vec!["shared"]);
    m.insert("deploy", vec!["infra", "deploy"]);
    m.insert("infra", vec!["infra"]);
    m.insert("ci", vec!["infra", "scripts"]);
    m.insert("docker", vec!["infra"]);
    m.insert("terraform", vec!["infra"]);
    m
});

/// Infer which scopes are relevant to a change description.
///
/// Uses keyword matching against known domains from the file index
/// and the keyword→domain map.
pub fn infer_scopes(
    change_description: &str,
    manifest: &[ManifestEntry],
    files: &[FileRecord],
) -> Vec<String> {
    let desc_lower = change_description.to_lowercase();

    // Collect all known domains
    let mut all_domains: HashSet<String> = HashSet::new();
    for f in files {
        if !f.domain.is_empty() {
            all_domains.insert(f.domain.clone());
        }
    }
    for entry in manifest {
        if !entry.scope.is_empty() {
            all_domains.insert(entry.scope.clone());
        }
    }

    let mut matched: Vec<String> = Vec::new();

    // Direct domain name match
    for domain in all_domains.iter() {
        if desc_lower.contains(&domain.to_lowercase()) && !matched.contains(domain) {
            matched.push(domain.clone());
        }
    }

    // Keyword mapping
    for (keyword, domains) in KEYWORD_MAP.iter() {
        if desc_lower.contains(keyword) {
            for &d in domains {
                let ds = d.to_string();
                if all_domains.contains(&ds) && !matched.contains(&ds) {
                    matched.push(ds);
                }
            }
        }
    }

    if matched.is_empty() {
        info!("No specific scopes matched — using root-level context only");
        matched.push("root".to_string());
    }

    matched.sort();
    info!(scopes = ?matched, "Inferred scopes for planning");
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_files(domains: &[&str]) -> Vec<FileRecord> {
        domains
            .iter()
            .map(|d| FileRecord {
                path: format!("apps/{d}/index.ts"),
                file_type: "module".to_string(),
                domain: d.to_string(),
                subdomain: "core".to_string(),
                claude_md: String::new(),
            })
            .collect()
    }

    fn make_manifest(scopes: &[&str]) -> Vec<ManifestEntry> {
        scopes
            .iter()
            .map(|s| ManifestEntry {
                id: format!("{s}-diagram"),
                source: format!("{s}/arch.mmd"),
                scope: s.to_string(),
                diagram_type: "flowchart".to_string(),
                tokens_est: 100,
            })
            .collect()
    }

    #[test]
    fn direct_domain_match() {
        let files = make_files(&["web", "admin", "supabase"]);
        let manifest = make_manifest(&["web", "admin"]);

        let scopes = infer_scopes("Update the web frontend", &manifest, &files);
        assert!(scopes.contains(&"web".to_string()));
    }

    #[test]
    fn keyword_database_matches_supabase() {
        let files = make_files(&["web", "admin", "supabase"]);
        let manifest = make_manifest(&[]);

        let scopes = infer_scopes("Add a database migration", &manifest, &files);
        assert!(scopes.contains(&"supabase".to_string()));
    }

    #[test]
    fn keyword_route_matches_web_admin() {
        let files = make_files(&["web", "admin"]);
        let manifest = make_manifest(&[]);

        let scopes = infer_scopes("Add a new route for user profile", &manifest, &files);
        assert!(scopes.contains(&"web".to_string()));
        assert!(scopes.contains(&"admin".to_string()));
    }

    #[test]
    fn keyword_deploy_matches_infra() {
        let files = make_files(&["web", "infra"]);
        let manifest = make_manifest(&[]);

        let scopes = infer_scopes("Fix the deploy pipeline", &manifest, &files);
        assert!(scopes.contains(&"infra".to_string()));
    }

    #[test]
    fn no_match_returns_root() {
        let files = make_files(&["web", "admin"]);
        let manifest = make_manifest(&[]);

        let scopes = infer_scopes("Something totally unrelated xyz", &manifest, &files);
        assert_eq!(scopes, vec!["root".to_string()]);
    }

    #[test]
    fn case_insensitive_matching() {
        let files = make_files(&["web", "supabase"]);
        let manifest = make_manifest(&[]);

        let scopes = infer_scopes("Add a DATABASE table", &manifest, &files);
        assert!(scopes.contains(&"supabase".to_string()));
    }

    #[test]
    fn multiple_keywords_match() {
        let files = make_files(&["web", "admin", "supabase", "shared"]);
        let manifest = make_manifest(&[]);

        let scopes = infer_scopes("Add auth types and database schema", &manifest, &files);
        assert!(scopes.contains(&"supabase".to_string()));
        assert!(scopes.contains(&"shared".to_string()));
    }

    #[test]
    fn manifest_scopes_are_considered() {
        let files = make_files(&["web"]);
        let manifest = make_manifest(&["api"]);

        let scopes = infer_scopes("Fix the api endpoint", &manifest, &files);
        assert!(scopes.contains(&"api".to_string()));
    }

    #[test]
    fn results_are_sorted() {
        let files = make_files(&["web", "admin", "supabase"]);
        let manifest = make_manifest(&[]);

        let scopes = infer_scopes("Update admin web supabase", &manifest, &files);
        let mut sorted = scopes.clone();
        sorted.sort();
        assert_eq!(scopes, sorted);
    }
}

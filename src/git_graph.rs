//! Generate Mermaid gitGraph diagrams from git history.

use std::path::Path;
use std::process::Command;
use tracing::info;

use crate::error::{Error, Result};

/// A parsed git commit.
#[derive(Debug, Clone)]
struct GitCommit {
    _hash: String,
    _short_hash: String,
    branch: String,
    message: String,
    is_merge: bool,
}

/// Generate a gitGraph Mermaid diagram from the git log.
pub fn generate_git_graph(root: &Path, limit: usize) -> Result<String> {
    let output = Command::new("git")
        .args([
            "log",
            "--all",
            "--oneline",
            "--graph",
            "--decorate=short",
            &format!("-{limit}"),
            "--format=%h|%D|%s|%P",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| Error::Config(format!("Failed to run git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Config(format!("git log failed: {stderr}")));
    }

    let log = String::from_utf8_lossy(&output.stdout);
    let commits = parse_git_log(&log);

    if commits.is_empty() {
        return Ok("gitGraph\n    commit id: \"no-history\"".to_string());
    }

    // Build gitGraph
    let mut lines = vec!["gitGraph".to_string()];
    let mut current_branch = "main".to_string();
    let mut known_branches = vec!["main".to_string()];

    for commit in commits.iter().rev() {
        // Branch detection
        if !commit.branch.is_empty() && commit.branch != current_branch {
            if !known_branches.contains(&commit.branch) {
                lines.push(format!("    branch {}", commit.branch));
                known_branches.push(commit.branch.clone());
            }
            lines.push(format!("    checkout {}", commit.branch));
            current_branch.clone_from(&commit.branch);
        }

        if commit.is_merge {
            // Find merge source from message
            let merge_source = extract_merge_source(&commit.message)
                .unwrap_or_else(|| "feature".to_string());
            if known_branches.contains(&merge_source) {
                lines.push(format!("    merge {merge_source}"));
            } else {
                lines.push(format!(
                    "    commit id: \"{}\" type: HIGHLIGHT",
                    sanitize_label(&commit.message)
                ));
            }
        } else {
            let label = sanitize_label(&commit.message);
            lines.push(format!("    commit id: \"{label}\""));
        }
    }

    Ok(lines.join("\n"))
}

fn parse_git_log(log: &str) -> Vec<GitCommit> {
    let mut commits = Vec::new();

    for line in log.lines() {
        // Strip graph decoration characters (*, |, /, \, _, space)
        let trimmed = line.trim_start_matches(|c: char| {
            c == '*' || c == '|' || c == '/' || c == '\\' || c == ' ' || c == '_'
        });
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.splitn(4, '|').collect();
        if parts.len() < 4 {
            continue;
        }

        let hash = parts[0].trim().to_string();
        let short_hash = hash.chars().take(7).collect();
        let refs = parts[1].trim();
        let message = parts[2].trim().to_string();
        let parents = parts[3].trim();

        let branch = extract_branch_from_refs(refs);
        let is_merge = parents.split_whitespace().count() > 1;

        commits.push(GitCommit {
            _hash: hash,
            _short_hash: short_hash,
            branch,
            message,
            is_merge,
        });
    }

    commits
}

fn extract_branch_from_refs(refs: &str) -> String {
    if refs.is_empty() {
        return String::new();
    }

    // Parse refs like "HEAD -> main, origin/main"
    for r in refs.split(',') {
        let r = r.trim();
        if let Some(branch) = r.strip_prefix("HEAD -> ") {
            return branch.to_string();
        }
    }

    // Fall back to first non-origin, non-tag ref
    for r in refs.split(',') {
        let r = r.trim();
        if !r.starts_with("origin/") && !r.starts_with("tag:") {
            return r.to_string();
        }
    }

    String::new()
}

fn extract_merge_source(message: &str) -> Option<String> {
    // "Merge branch 'feature/xyz' into main"
    if let Some(start) = message.find('\'') {
        if let Some(end) = message[start + 1..].find('\'') {
            return Some(message[start + 1..start + 1 + end].to_string());
        }
    }
    // "Merge pull request #123 from user/branch"
    if let Some(idx) = message.find("from ") {
        let branch = message[idx + 5..].split_whitespace().next()?;
        return Some(branch.split('/').next_back()?.to_string());
    }
    None
}

fn sanitize_label(msg: &str) -> String {
    msg.chars()
        .take(40)
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_' || *c == '.')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Write the git graph diagram to a file.
pub fn write_git_graph(root: &Path, limit: usize, output: Option<&str>) -> Result<()> {
    let diagram = generate_git_graph(root, limit)?;

    let output_path = match output {
        Some(p) => root.join(p),
        None => {
            let dir = crate::config::output_dir(root)
                .join("diagrams")
                .join("extracted");
            std::fs::create_dir_all(&dir).map_err(|e| crate::error::io_err(&dir, e))?;
            dir.join("git-history.mmd")
        }
    };

    std::fs::write(&output_path, &diagram)
        .map_err(|e| crate::error::io_err(&output_path, e))?;

    info!(
        path = %output_path.display(),
        commits = limit,
        "Wrote git graph diagram"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_label_removes_special_chars() {
        assert_eq!(
            sanitize_label("feat: add (new) thing"),
            "feat add new thing"
        );
    }

    #[test]
    fn sanitize_label_truncates() {
        let long = "a".repeat(100);
        assert_eq!(sanitize_label(&long).len(), 40);
    }

    #[test]
    fn extract_merge_branch_single_quotes() {
        assert_eq!(
            extract_merge_source("Merge branch 'feature/auth' into main"),
            Some("feature/auth".to_string())
        );
    }

    #[test]
    fn extract_merge_branch_from() {
        assert_eq!(
            extract_merge_source("Merge pull request #42 from user/fix-bug"),
            Some("fix-bug".to_string())
        );
    }

    #[test]
    fn extract_merge_no_match() {
        assert_eq!(extract_merge_source("Initial commit"), None);
    }

    #[test]
    fn extract_branch_from_refs_head() {
        assert_eq!(
            extract_branch_from_refs("HEAD -> main, origin/main"),
            "main".to_string()
        );
    }

    #[test]
    fn extract_branch_from_refs_empty() {
        assert_eq!(extract_branch_from_refs(""), String::new());
    }

    #[test]
    fn extract_branch_from_refs_no_head() {
        // Falls back to first non-origin ref
        assert_eq!(
            extract_branch_from_refs("feature/x, origin/feature/x"),
            "feature/x".to_string()
        );
    }

    #[test]
    fn parse_git_log_empty() {
        let commits = parse_git_log("");
        assert!(commits.is_empty());
    }

    #[test]
    fn parse_git_log_single_commit() {
        let log = "* abc1234|HEAD -> main|Initial commit|";
        let commits = parse_git_log(log);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].branch, "main");
        assert_eq!(commits[0].message, "Initial commit");
        assert!(!commits[0].is_merge);
    }

    #[test]
    fn parse_git_log_merge_commit() {
        let log = "* abc1234|HEAD -> main|Merge branch 'feat' into main|def5678 ghi9012";
        let commits = parse_git_log(log);
        assert_eq!(commits.len(), 1);
        assert!(commits[0].is_merge);
    }

    #[test]
    fn generate_git_graph_no_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = generate_git_graph(dir.path(), 50);
        // git log should fail on a non-repo directory
        assert!(result.is_err());
    }
}

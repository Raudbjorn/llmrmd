//! Git hook management for automatic diagram regeneration.

use std::path::Path;
use tracing::info;

use crate::error::{self, Error, Result};

/// The pre-commit hook script content.
const PRE_COMMIT_HOOK: &str = r#"#!/bin/sh
# llmermaid pre-commit hook: regenerate diagrams for changed files
# Installed by `llmermaid init-hook`

set -e

# Check if llmermaid is available
if ! command -v llmermaid >/dev/null 2>&1; then
    echo "llmermaid not found in PATH — skipping diagram regeneration"
    exit 0
fi

# Get list of staged files
STAGED=$(git diff --cached --name-only --diff-filter=ACM)

if [ -z "$STAGED" ]; then
    exit 0
fi

# Check if any source files changed (not just diagrams)
HAS_SOURCE=false
for f in $STAGED; do
    case "$f" in
        *.rs|*.ts|*.tsx|*.js|*.jsx|*.py|*.go|*.svelte|*.vue|*.md|*.mmd)
            HAS_SOURCE=true
            break
            ;;
    esac
done

if [ "$HAS_SOURCE" = "true" ]; then
    echo "llmermaid: regenerating diagrams..."
    llmermaid index --skip-checks 2>/dev/null || true

    # Stage any updated diagram files
    if [ -d ".claude/diagrams" ]; then
        git add .claude/diagrams/ .claude/manifest.* .claude/file-index.* 2>/dev/null || true
    fi
fi
"#;

/// Install the pre-commit hook into the repo's `.git/hooks/` directory.
///
/// If a pre-commit hook already exists and contains "llmermaid", this is a
/// no-op (idempotent). If a *different* hook exists, the caller must pass
/// `force = true` to overwrite it.
pub fn install_pre_commit(root: &Path, force: bool) -> Result<()> {
    let git_dir = root.join(".git");
    if !git_dir.exists() {
        return Err(Error::Config(
            "Not a git repository (no .git directory found)".to_string(),
        ));
    }

    let hooks_dir = git_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).map_err(|e| error::io_err(&hooks_dir, e))?;

    let hook_path = hooks_dir.join("pre-commit");

    if hook_path.exists() && !force {
        // Check if it's our hook — if so, nothing to do.
        let existing =
            std::fs::read_to_string(&hook_path).map_err(|e| error::io_err(&hook_path, e))?;
        if existing.contains("llmermaid") {
            info!("llmermaid pre-commit hook already installed");
            return Ok(());
        }
        return Err(Error::Config(format!(
            "Pre-commit hook already exists at {}. Use --force to overwrite.",
            hook_path.display()
        )));
    }

    std::fs::write(&hook_path, PRE_COMMIT_HOOK)
        .map_err(|e| error::io_err(&hook_path, e))?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)
            .map_err(|e| error::io_err(&hook_path, e))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms)
            .map_err(|e| error::io_err(&hook_path, e))?;
    }

    info!(path = %hook_path.display(), "Installed pre-commit hook");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_fails_without_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = install_pre_commit(dir.path(), false);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Not a git repository"));
    }

    #[test]
    fn install_creates_hook_file() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();

        install_pre_commit(dir.path(), false).unwrap();

        let hook = git_dir.join("hooks").join("pre-commit");
        assert!(hook.exists());

        let content = std::fs::read_to_string(&hook).unwrap();
        assert!(content.contains("llmermaid"));
        assert!(content.starts_with("#!/bin/sh"));
    }

    #[test]
    fn install_idempotent_when_already_installed() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();

        install_pre_commit(dir.path(), false).unwrap();
        // Second call should succeed (idempotent).
        install_pre_commit(dir.path(), false).unwrap();
    }

    #[test]
    fn install_refuses_to_overwrite_foreign_hook() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".git").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();

        let hook_path = hooks_dir.join("pre-commit");
        std::fs::write(&hook_path, "#!/bin/sh\necho 'other tool'").unwrap();

        let result = install_pre_commit(dir.path(), false);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("--force"));
    }

    #[test]
    fn install_force_overwrites_foreign_hook() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join(".git").join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();

        let hook_path = hooks_dir.join("pre-commit");
        std::fs::write(&hook_path, "#!/bin/sh\necho 'other tool'").unwrap();

        install_pre_commit(dir.path(), true).unwrap();

        let content = std::fs::read_to_string(&hook_path).unwrap();
        assert!(content.contains("llmermaid"));
    }

    #[cfg(unix)]
    #[test]
    fn install_sets_executable_permission() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();

        install_pre_commit(dir.path(), false).unwrap();

        let hook = git_dir.join("hooks").join("pre-commit");
        let mode = std::fs::metadata(&hook).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "Hook should be executable");
    }
}

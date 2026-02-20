//! Plugin registry -- discovers and manages language plugins.
//!
//! The registry holds all known [`LanguagePlugin`] implementations and provides
//! methods to query by extension, filter by available prerequisites, and
//! batch-generate diagrams across all matching plugins.

use std::path::Path;

use tracing::{info, warn};

use super::{GeneratedDiagram, LanguagePlugin};
use crate::error::Result;

/// Registry of all available language plugins.
pub struct PluginRegistry {
    plugins: Vec<Box<dyn LanguagePlugin>>,
}

impl PluginRegistry {
    /// Create a new registry with all built-in plugins registered.
    pub fn new() -> Self {
        let mut registry = Self {
            plugins: Vec::new(),
        };

        // Register built-in plugins in order of typical monorepo prevalence.
        registry.register(Box::new(super::builtin::RustPlugin));
        registry.register(Box::new(super::builtin::TypeScriptPlugin));
        registry.register(Box::new(super::builtin::PythonPlugin));
        registry.register(Box::new(super::builtin::GoPlugin));

        registry
    }

    /// Register an additional plugin.
    pub fn register(&mut self, plugin: Box<dyn LanguagePlugin>) {
        info!(
            name = plugin.name(),
            extensions = ?plugin.extensions(),
            "Registered language plugin"
        );
        self.plugins.push(plugin);
    }

    /// Return all registered plugins (regardless of prerequisites).
    pub fn all_plugins(&self) -> &[Box<dyn LanguagePlugin>] {
        &self.plugins
    }

    /// Find plugins that handle a given file extension (e.g., ".rs").
    pub fn plugins_for_extension(&self, ext: &str) -> Vec<&dyn LanguagePlugin> {
        self.plugins
            .iter()
            .filter(|p| p.extensions().contains(&ext))
            .map(|p| p.as_ref())
            .collect()
    }

    /// Get all plugins whose prerequisites are satisfied.
    ///
    /// Plugins that fail the prerequisite check are logged and skipped rather
    /// than causing a hard error -- graceful degradation.
    pub fn available_plugins(&self) -> Vec<&dyn LanguagePlugin> {
        self.plugins
            .iter()
            .filter(|p| match p.check_prerequisites() {
                Ok(true) => true,
                Ok(false) => {
                    info!(
                        name = p.name(),
                        "Plugin prerequisites not met -- skipping"
                    );
                    false
                }
                Err(e) => {
                    warn!(
                        name = p.name(),
                        error = %e,
                        "Plugin prerequisite check failed"
                    );
                    false
                }
            })
            .map(|p| p.as_ref())
            .collect()
    }

    /// Generate diagrams from all available plugins for files in the repo.
    ///
    /// `files` is a slice of `(relative_path, extension)` pairs. Each plugin
    /// receives only the files matching its declared extensions.
    ///
    /// Returns `(plugin_name, diagram)` pairs. Plugin failures are logged as
    /// warnings but do not abort the overall generation.
    pub fn generate_all(
        &self,
        root: &Path,
        files: &[(String, String)],
    ) -> Result<Vec<(String, GeneratedDiagram)>> {
        let mut results = Vec::new();

        for plugin in self.available_plugins() {
            let matching: Vec<&Path> = files
                .iter()
                .filter(|(_, ext)| plugin.extensions().contains(&ext.as_str()))
                .map(|(path, _)| Path::new(path.as_str()))
                .collect();

            if matching.is_empty() {
                continue;
            }

            info!(
                plugin = plugin.name(),
                files = matching.len(),
                "Generating diagrams"
            );

            match plugin.generate(root, &matching) {
                Ok(diagrams) => {
                    for diag in diagrams {
                        results.push((plugin.name().to_string(), diag));
                    }
                }
                Err(e) => {
                    warn!(
                        plugin = plugin.name(),
                        error = %e,
                        "Plugin generation failed"
                    );
                }
            }
        }

        Ok(results)
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{DiagramLevel, GeneratedDiagram};
    use std::path::Path;

    /// A minimal test plugin that always succeeds.
    struct TestPlugin;

    impl LanguagePlugin for TestPlugin {
        fn name(&self) -> &str {
            "test"
        }

        fn extensions(&self) -> &[&str] {
            &[".test"]
        }

        fn check_prerequisites(&self) -> crate::error::Result<bool> {
            Ok(true)
        }

        fn generate(
            &self,
            _root: &Path,
            files: &[&Path],
        ) -> crate::error::Result<Vec<GeneratedDiagram>> {
            Ok(vec![GeneratedDiagram {
                content: format!("flowchart LR\n  files[{} files]", files.len()),
                level: DiagramLevel::Class,
                diagram_type: "flowchart".to_string(),
                label: "Test diagram".to_string(),
            }])
        }
    }

    /// A test plugin whose prerequisites are never met.
    struct UnavailablePlugin;

    impl LanguagePlugin for UnavailablePlugin {
        fn name(&self) -> &str {
            "unavailable"
        }

        fn extensions(&self) -> &[&str] {
            &[".nope"]
        }

        fn check_prerequisites(&self) -> crate::error::Result<bool> {
            Ok(false)
        }

        fn generate(
            &self,
            _root: &Path,
            _files: &[&Path],
        ) -> crate::error::Result<Vec<GeneratedDiagram>> {
            unreachable!("should never be called when prerequisites are not met");
        }
    }

    /// A test plugin whose prerequisite check errors.
    struct ErrorPlugin;

    impl LanguagePlugin for ErrorPlugin {
        fn name(&self) -> &str {
            "error"
        }

        fn extensions(&self) -> &[&str] {
            &[".err"]
        }

        fn check_prerequisites(&self) -> crate::error::Result<bool> {
            Err(crate::error::Error::Plugin {
                plugin: "error".to_string(),
                message: "check exploded".to_string(),
            })
        }

        fn generate(
            &self,
            _root: &Path,
            _files: &[&Path],
        ) -> crate::error::Result<Vec<GeneratedDiagram>> {
            unreachable!("should never be called when prerequisites errored");
        }
    }

    #[test]
    fn default_registry_has_builtin_plugins() {
        let reg = PluginRegistry::default();
        let names: Vec<&str> = reg.all_plugins().iter().map(|p| p.name()).collect();
        assert!(names.contains(&"rust"));
        assert!(names.contains(&"typescript"));
        assert!(names.contains(&"python"));
        assert!(names.contains(&"go"));
    }

    #[test]
    fn register_custom_plugin() {
        let mut reg = PluginRegistry {
            plugins: Vec::new(),
        };
        reg.register(Box::new(TestPlugin));
        assert_eq!(reg.all_plugins().len(), 1);
        assert_eq!(reg.all_plugins()[0].name(), "test");
    }

    #[test]
    fn plugins_for_extension_filters_correctly() {
        let mut reg = PluginRegistry {
            plugins: Vec::new(),
        };
        reg.register(Box::new(TestPlugin));
        reg.register(Box::new(UnavailablePlugin));

        let test_matches = reg.plugins_for_extension(".test");
        assert_eq!(test_matches.len(), 1);
        assert_eq!(test_matches[0].name(), "test");

        let nope_matches = reg.plugins_for_extension(".nope");
        assert_eq!(nope_matches.len(), 1);
        assert_eq!(nope_matches[0].name(), "unavailable");

        let none_matches = reg.plugins_for_extension(".xyz");
        assert!(none_matches.is_empty());
    }

    #[test]
    fn available_plugins_filters_unavailable() {
        let mut reg = PluginRegistry {
            plugins: Vec::new(),
        };
        reg.register(Box::new(TestPlugin));
        reg.register(Box::new(UnavailablePlugin));
        reg.register(Box::new(ErrorPlugin));

        let available = reg.available_plugins();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].name(), "test");
    }

    #[test]
    fn generate_all_with_matching_files() {
        let mut reg = PluginRegistry {
            plugins: Vec::new(),
        };
        reg.register(Box::new(TestPlugin));

        let files = vec![
            ("src/foo.test".to_string(), ".test".to_string()),
            ("src/bar.test".to_string(), ".test".to_string()),
            ("src/baz.other".to_string(), ".other".to_string()),
        ];

        let results = reg.generate_all(Path::new("/tmp"), &files).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "test");
        assert!(results[0].1.content.contains("2 files"));
    }

    #[test]
    fn generate_all_no_matching_files() {
        let mut reg = PluginRegistry {
            plugins: Vec::new(),
        };
        reg.register(Box::new(TestPlugin));

        let files = vec![("src/foo.rs".to_string(), ".rs".to_string())];

        let results = reg.generate_all(Path::new("/tmp"), &files).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn generate_all_skips_unavailable_plugins() {
        let mut reg = PluginRegistry {
            plugins: Vec::new(),
        };
        reg.register(Box::new(UnavailablePlugin));

        let files = vec![("src/foo.nope".to_string(), ".nope".to_string())];

        let results = reg.generate_all(Path::new("/tmp"), &files).unwrap();
        assert!(results.is_empty());
    }
}

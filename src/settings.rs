//! Runtime configuration loaded from `llmermaid.toml`.
//!
//! Configuration is loaded from (in priority order):
//! 1. `<repo>/.llmermaid.toml` (project-level)
//! 2. `~/.config/llmermaid/config.toml` (user-level)
//! 3. Built-in defaults

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Complete configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// General settings.
    pub general: GeneralSettings,
    /// TUI theme settings.
    pub theme: ThemeSettings,
    /// Indexer settings.
    pub indexer: IndexerSettings,
    /// Planner settings.
    pub planner: PlannerSettings,
    /// Plugin settings.
    pub plugins: PluginSettings,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// Default editor command (falls back to $EDITOR, then vi).
    pub editor: Option<String>,
    /// Whether to run preflight checks by default.
    pub skip_checks: bool,
    /// Verbose logging.
    pub verbose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeSettings {
    /// Theme name: "dark", "light", or "auto".
    pub name: String,
    /// Custom color overrides.
    pub colors: ThemeColors,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeColors {
    /// Primary accent color (for selected items, tab highlights).
    pub accent: String,
    /// Secondary color (for labels, titles).
    pub secondary: String,
    /// Success indicator color.
    pub success: String,
    /// Warning indicator color.
    pub warning: String,
    /// Error indicator color.
    pub error: String,
    /// Muted/dim text color.
    pub muted: String,
    /// Background color for selected items.
    pub selection_bg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexerSettings {
    /// Additional directories to skip during scanning.
    pub extra_skip_dirs: Vec<String>,
    /// Additional file extensions to skip.
    pub extra_skip_extensions: Vec<String>,
    /// Whether to generate hierarchical diagrams.
    pub hierarchical_diagrams: bool,
    /// Enable mermaid syntax repair on extraction.
    pub auto_repair: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlannerSettings {
    /// Default token budget.
    pub token_budget: usize,
    /// Whether to use semantic XML tags in output.
    pub semantic_tags: bool,
    /// Export filename pattern.
    pub export_filename: String,
    /// Default export directory (relative to repo root).
    pub export_dir: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    /// Enable/disable specific language plugins.
    pub enabled: Vec<String>,
    /// Disable specific language plugins.
    pub disabled: Vec<String>,
}

// -- Non-trivial Defaults --

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            name: "dark".to_string(),
            colors: ThemeColors::default(),
        }
    }
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            accent: "cyan".to_string(),
            secondary: "yellow".to_string(),
            success: "green".to_string(),
            warning: "yellow".to_string(),
            error: "red".to_string(),
            muted: "dark_gray".to_string(),
            selection_bg: "dark_gray".to_string(),
        }
    }
}

impl Default for IndexerSettings {
    fn default() -> Self {
        Self {
            extra_skip_dirs: Vec::new(),
            extra_skip_extensions: Vec::new(),
            hierarchical_diagrams: false,
            auto_repair: true,
        }
    }
}

impl Default for PlannerSettings {
    fn default() -> Self {
        Self {
            token_budget: 4000,
            semantic_tags: true,
            export_filename: "planner-context.md".to_string(),
            export_dir: ".claude".to_string(),
        }
    }
}

// -- Loading --

/// Load settings from config files.
pub fn load_settings(root: &Path) -> Settings {
    // Try project-level config first
    let project_config = root.join(".llmermaid.toml");
    if let Some(settings) = try_load(&project_config) {
        info!(path = %project_config.display(), "Loaded project config");
        return settings;
    }

    // Try user-level config
    if let Some(home) = dirs_path() {
        let user_config = home.join("config.toml");
        if let Some(settings) = try_load(&user_config) {
            info!(path = %user_config.display(), "Loaded user config");
            return settings;
        }
    }

    info!("Using default settings");
    Settings::default()
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config").join("llmermaid"))
}

fn try_load(path: &Path) -> Option<Settings> {
    let content = std::fs::read_to_string(path).ok()?;
    match toml::from_str::<Settings>(&content) {
        Ok(settings) => Some(settings),
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Failed to parse config");
            None
        }
    }
}

/// Generate a default config file content for reference.
pub fn default_config_toml() -> String {
    let settings = Settings::default();
    toml::to_string_pretty(&settings)
        .unwrap_or_else(|_| "# Failed to generate defaults".to_string())
}

// -- Theme resolution --

impl ThemeColors {
    /// Resolve a color name to a ratatui Color.
    pub fn resolve_color(name: &str) -> ratatui::style::Color {
        use ratatui::style::Color;
        match name.to_lowercase().as_str() {
            "black" => Color::Black,
            "red" => Color::Red,
            "green" => Color::Green,
            "yellow" => Color::Yellow,
            "blue" => Color::Blue,
            "magenta" => Color::Magenta,
            "cyan" => Color::Cyan,
            "white" => Color::White,
            "gray" | "grey" => Color::Gray,
            "dark_gray" | "dark_grey" | "darkgray" => Color::DarkGray,
            "light_red" | "lightred" => Color::LightRed,
            "light_green" | "lightgreen" => Color::LightGreen,
            "light_yellow" | "lightyellow" => Color::LightYellow,
            "light_blue" | "lightblue" => Color::LightBlue,
            "light_magenta" | "lightmagenta" => Color::LightMagenta,
            "light_cyan" | "lightcyan" => Color::LightCyan,
            _ => {
                // Try hex color
                if name.starts_with('#') && name.len() == 7 {
                    let r = u8::from_str_radix(&name[1..3], 16).unwrap_or(0);
                    let g = u8::from_str_radix(&name[3..5], 16).unwrap_or(0);
                    let b = u8::from_str_radix(&name[5..7], 16).unwrap_or(0);
                    Color::Rgb(r, g, b)
                } else {
                    Color::White
                }
            }
        }
    }

    pub fn accent_color(&self) -> ratatui::style::Color {
        Self::resolve_color(&self.accent)
    }

    pub fn secondary_color(&self) -> ratatui::style::Color {
        Self::resolve_color(&self.secondary)
    }

    pub fn success_color(&self) -> ratatui::style::Color {
        Self::resolve_color(&self.success)
    }

    pub fn warning_color(&self) -> ratatui::style::Color {
        Self::resolve_color(&self.warning)
    }

    pub fn error_color(&self) -> ratatui::style::Color {
        Self::resolve_color(&self.error)
    }

    pub fn muted_color(&self) -> ratatui::style::Color {
        Self::resolve_color(&self.muted)
    }

    pub fn selection_bg_color(&self) -> ratatui::style::Color {
        Self::resolve_color(&self.selection_bg)
    }
}

/// Pre-built light theme.
pub fn light_theme() -> ThemeColors {
    ThemeColors {
        accent: "blue".to_string(),
        secondary: "magenta".to_string(),
        success: "green".to_string(),
        warning: "yellow".to_string(),
        error: "red".to_string(),
        muted: "gray".to_string(),
        selection_bg: "light_blue".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_valid() {
        let settings = Settings::default();
        assert_eq!(settings.theme.name, "dark");
        assert_eq!(settings.planner.token_budget, 4000);
        assert!(settings.indexer.auto_repair);
    }

    #[test]
    fn parse_minimal_toml() {
        let toml_str = r#"
[general]
verbose = true

[planner]
token_budget = 8000
"#;
        let settings: Settings = toml::from_str(toml_str).unwrap();
        assert!(settings.general.verbose);
        assert_eq!(settings.planner.token_budget, 8000);
        // Defaults preserved
        assert_eq!(settings.theme.name, "dark");
    }

    #[test]
    fn parse_theme_with_hex_colors() {
        let toml_str = r##"
[theme.colors]
accent = "#00ff88"
"##;
        let settings: Settings = toml::from_str(toml_str).unwrap();
        let color = settings.theme.colors.accent_color();
        assert!(matches!(color, ratatui::style::Color::Rgb(0, 255, 136)));
    }

    #[test]
    fn resolve_color_names() {
        assert_eq!(
            ThemeColors::resolve_color("cyan"),
            ratatui::style::Color::Cyan
        );
        assert_eq!(
            ThemeColors::resolve_color("dark_gray"),
            ratatui::style::Color::DarkGray
        );
        assert_eq!(
            ThemeColors::resolve_color("unknown"),
            ratatui::style::Color::White
        );
    }

    #[test]
    fn default_config_toml_is_valid() {
        let toml_str = default_config_toml();
        let _: Settings = toml::from_str(&toml_str).unwrap();
    }

    #[test]
    fn load_settings_returns_defaults_when_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let settings = load_settings(dir.path());
        assert_eq!(settings.planner.token_budget, 4000);
    }

    #[test]
    fn load_settings_from_project_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".llmermaid.toml"),
            "[planner]\ntoken_budget = 6000\n",
        )
        .unwrap();

        let settings = load_settings(dir.path());
        assert_eq!(settings.planner.token_budget, 6000);
    }

    #[test]
    fn light_theme_colors() {
        let theme = light_theme();
        assert_eq!(theme.accent, "blue");
        assert_eq!(
            ThemeColors::resolve_color(&theme.accent),
            ratatui::style::Color::Blue
        );
    }
}

//! Unified configuration for the banshee SQL toolchain.
//!
//! [`BansheeConfig`] is the single struct threaded through the formatter, linter,
//! completion engine and the generic LSP. It deserializes from a `banshee.toml`
//! file whose schema is deliberately decoupled from the internal types of each
//! crate, so the file format can stay stable as internals evolve.
//!
//! # Example `banshee.toml`
//!
//! ```toml
//! [format]
//! style = "sqlstyle"
//! keyword-case = "upper"
//! max-width = 100
//!
//! [lint]
//! enabled = true
//! exclude = ["AM01"]
//!
//! [lint.rules.CP01]
//! severity = "warning"
//!
//! [database]
//! url-env = "DATABASE_URL"
//! schema = "public"
//! ```

mod analysis;
mod database;
pub mod fixes;
mod format;
mod formatting;
mod lint;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use analysis::ConfiguredAnalysis;
pub use banshee_templater::{PlaceholderStyle, Templated};
pub use database::DatabaseSettings;
pub use format::{
    CasePolicy, CommaStyle, FormatSettings, IdentifierCase, IndentUnit, KeywordCase,
    PgFormatSettings, StylePreset,
};
pub use lint::{LintSettings, RuleSetting, SeverityLevel};

/// Standard config file name discovered by [`BansheeConfig::discover`].
pub const CONFIG_FILE_NAME: &str = "banshee.toml";

/// Completion engine settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct CompletionSettings {
    /// Suggest SQL keywords.
    pub keywords: bool,
    /// Suggest functions.
    pub functions: bool,
    /// Maximum number of items returned per request (0 = unlimited).
    pub max_items: usize,
}

impl Default for CompletionSettings {
    fn default() -> Self {
        Self {
            keywords: true,
            functions: true,
            max_items: 200,
        }
    }
}

/// Top-level configuration aggregating every subsystem's settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct BansheeConfig {
    pub format: FormatSettings,
    pub lint: LintSettings,
    pub completion: CompletionSettings,
    pub database: DatabaseSettings,
    pub templater: TemplaterSettings,
}

/// Placeholder templating settings.
///
/// When `style` is set, parameter placeholders are substituted before parsing
/// so that tools can analyze app SQL (`:name`, `?`, `%(name)s`). Off by default.
///
/// ```toml
/// [templater]
/// style = "colon"   # colon | question-mark | percent
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct TemplaterSettings {
    /// The placeholder syntax to substitute; `None` disables templating.
    pub style: Option<banshee_templater::PlaceholderStyle>,
}

impl TemplaterSettings {
    /// Renders SQL for analysis when the configured style makes a substitution.
    pub fn render(&self, source: &str) -> Option<Templated> {
        let rendered = banshee_templater::render(source, self.style?);
        (rendered.text() != source).then_some(rendered)
    }

    /// Parses native SQL first and uses the configured template if needed.
    /// The returned map translates the chosen parse's offsets to `source`.
    pub fn parse(&self, source: &str) -> (banshee_syntax::Parse, Option<Templated>) {
        let native = banshee_parser::parse(source);
        match self.render_on_error(source, &native) {
            Some(template) => (banshee_parser::parse(template.text()), Some(template)),
            None => (native, None),
        }
    }

    pub(crate) fn render_on_error(
        &self,
        source: &str,
        native: &banshee_syntax::Parse,
    ) -> Option<Templated> {
        if native.errors().is_empty() {
            None
        } else {
            self.render(source)
        }
    }
}

/// Errors raised while loading configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The file contents were not valid TOML for this schema.
    Parse(toml::de::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "failed to read config: {e}"),
            ConfigError::Parse(e) => write!(f, "invalid config: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io(e) => Some(e),
            ConfigError::Parse(e) => Some(e),
        }
    }
}

impl BansheeConfig {
    /// Parses configuration from a TOML string.
    pub fn from_toml(src: &str) -> Result<Self, ConfigError> {
        toml::from_str(src).map_err(ConfigError::Parse)
    }

    /// Loads configuration from an explicit file path.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let src = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        Self::from_toml(&src)
    }

    /// Walks up from `start` looking for a `banshee.toml`.
    ///
    /// Returns the located path without parsing it. Useful for diagnostics
    /// ("using config at …"). `start` may be a file or a directory.
    #[must_use]
    pub fn find_config_file(start: &Path) -> Option<PathBuf> {
        let mut dir = if start.is_file() {
            start.parent()?.to_path_buf()
        } else {
            start.to_path_buf()
        };
        loop {
            let candidate = dir.join(CONFIG_FILE_NAME);
            if candidate.is_file() {
                return Some(candidate);
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    /// Discovers and loads the nearest `banshee.toml` walking up from `start`.
    ///
    /// Falls back to [`BansheeConfig::default`] when no file is found. A malformed
    /// file surfaces as an error rather than being silently ignored.
    pub fn discover(start: &Path) -> Result<Self, ConfigError> {
        match Self::find_config_file(start) {
            Some(path) => Self::load(&path),
            None => Ok(Self::default()),
        }
    }

    /// Resolves the sqlstyle-engine formatter configuration.
    pub fn format_config(&self) -> banshee_format::FormatConfig {
        self.format.to_format_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_is_all_defaults() {
        let cfg = BansheeConfig::from_toml("").unwrap();
        assert!(cfg.lint.enabled);
        assert!(!cfg.database.is_configured());
        assert_eq!(cfg.completion.max_items, 200);
    }

    #[test]
    fn parses_full_config() {
        let src = r#"
            [format]
            style = "compact"
            keyword-case = "lower"
            max-width = 100

            [lint]
            exclude = ["AM01"]

            [lint.rules.CP01]
            severity = "warning"

            [database]
            url-env = "DATABASE_URL"
            schema = "app"
        "#;
        let cfg = BansheeConfig::from_toml(src).unwrap();
        assert_eq!(cfg.format.style, StylePreset::Compact);
        assert_eq!(cfg.format.max_width, Some(100));
        assert!(!cfg.lint.is_rule_enabled("AM01"));
        assert_eq!(cfg.lint.severity_for("CP01"), Some(SeverityLevel::Warning));
        assert_eq!(cfg.database.schema, "app");
        assert_eq!(cfg.database.url_env.as_deref(), Some("DATABASE_URL"));
    }

    #[test]
    fn format_config_reflects_overrides() {
        let cfg = BansheeConfig::from_toml(
            r#"
            [format]
            style = "sqlstyle"
            keyword-case = "lower"
        "#,
        )
        .unwrap();
        let fc = cfg.format_config();
        assert_eq!(fc.keyword_case, banshee_format::KeywordCase::Lower);
    }

    #[test]
    fn range_formatting_uses_only_the_selected_statement_and_configured_preset() {
        let cfg = BansheeConfig::from_toml("[format]\nstyle = \"pgformatter\"\n").unwrap();
        let source = "select 1;\nselect 2;\nselect 3;\n";
        let edits = cfg.format_range(source, 10, 19);
        let [edit] = edits.as_slice() else {
            panic!("expected a single edit, got {edits:?}");
        };
        let mut output = source.to_owned();
        output.replace_range(std::ops::Range::<usize>::from(edit.range), &edit.new_text);
        assert_eq!(output, "select 1;\nSELECT\n    2;\nselect 3;\n");
        assert!(cfg.format_range(source, 19, 10).is_empty());
        assert!(cfg.format_range(source, 10, 10).is_empty());
    }

    #[test]
    fn rejects_unknown_severity() {
        let err = BansheeConfig::from_toml(
            r#"
            [lint.rules.CP01]
            severity = "bogus"
        "#,
        );
        assert!(err.is_err());
    }
}

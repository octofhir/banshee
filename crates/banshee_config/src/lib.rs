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

mod database;
mod format;
mod lint;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use banshee_templater::PlaceholderStyle;
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
/// so that app SQL (`:name`, `?`, `%(name)s`) can be linted. Off by default.
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

    /// Formats `source` with the engine selected by `format.style`.
    ///
    /// `pgformatter` routes through the pgFormatter engine; every other preset
    /// uses the sqlstyle engine. This is the single entry point callers should
    /// use so both formatting styles are honored.
    #[must_use]
    pub fn format(&self, source: &str) -> String {
        self.try_format(source)
            .unwrap_or_else(|_| source.to_owned())
    }

    /// Formats source with the configured engine and reports formatting failures.
    pub fn try_format(&self, source: &str) -> Result<String, banshee_format::FormatError> {
        match self.format.style {
            StylePreset::Pgformatter => {
                banshee_format::pg_format::try_format(source, &self.format.to_pg_config())
            }
            _ => banshee_format::try_format(source, &self.format.to_format_config()),
        }
    }

    /// Formats `source` and returns the minimal edits to apply, regardless of
    /// which engine the style selects.
    #[must_use]
    pub fn format_edits(&self, source: &str) -> Vec<banshee_format::TextEdit> {
        banshee_format::diff_edits(source, &self.format(source))
    }

    /// Like [`BansheeConfig::format_edits`], but only edits intersecting the byte
    /// range `[start, end]`.
    #[must_use]
    pub fn format_range(
        &self,
        source: &str,
        start: u32,
        end: u32,
    ) -> Vec<banshee_format::TextEdit> {
        self.format_edits(source)
            .into_iter()
            .filter(|e| u32::from(e.range.start()) <= end && start <= u32::from(e.range.end()))
            .collect()
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

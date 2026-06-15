//! Lint rule selection and per-rule configuration.
//!
//! Decoupled from `banshee_hir`'s runtime lint types: this is the on-disk schema.
//! Consumers translate [`SeverityLevel`] into `banshee_hir::Severity` and resolve
//! the effective rule set at analysis time.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Per-rule severity, including a disabled state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SeverityLevel {
    Error,
    Warning,
    Info,
    /// Rule is disabled.
    Off,
}

/// Configuration for a single rule, keyed by its code (e.g. `RF01`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct RuleSetting {
    /// Explicit enable/disable override. `None` defers to `select`/`exclude`.
    pub enabled: Option<bool>,
    /// Severity override for this rule's diagnostics.
    pub severity: Option<SeverityLevel>,
    /// Free-form, rule-specific options (e.g. `capitalisation-policy`).
    #[serde(flatten)]
    pub options: BTreeMap<String, toml::Value>,
}

/// Lint settings: global toggle, rule selection, and per-rule overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct LintSettings {
    /// Master switch for linting.
    pub enabled: bool,
    /// Rule codes or pack names to enable. Empty means "the default packs".
    pub select: Vec<String>,
    /// Rule codes or pack names to exclude, applied after `select`.
    pub exclude: Vec<String>,
    /// Glob patterns for files to skip entirely (e.g. legacy migrations).
    /// Matched against each input path; matching files are not analyzed.
    pub exclude_paths: Vec<String>,
    /// Per-rule configuration keyed by rule code.
    pub rules: BTreeMap<String, RuleSetting>,
}

impl Default for LintSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            select: Vec::new(),
            exclude: Vec::new(),
            exclude_paths: Vec::new(),
            rules: BTreeMap::new(),
        }
    }
}

impl LintSettings {
    /// Whether a given rule code is active under the current selection.
    ///
    /// Resolution order: explicit per-rule `enabled` > `exclude` > `select`
    /// (empty `select` = all default rules on).
    #[must_use]
    pub fn is_rule_enabled(&self, code: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(setting) = self.rules.get(code)
            && let Some(enabled) = setting.enabled
        {
            return enabled;
        }
        if self.exclude.iter().any(|c| c == code) {
            return false;
        }
        if self.select.is_empty() {
            return true;
        }
        self.select.iter().any(|c| c == code)
    }

    /// Effective severity override for a rule, if any.
    #[must_use]
    pub fn severity_for(&self, code: &str) -> Option<SeverityLevel> {
        self.rules.get(code).and_then(|r| r.severity)
    }

    /// Per-rule options stringified for the analyzer: `code -> (key -> value)`.
    /// Arrays are comma-joined; non-string scalars use their display form.
    #[must_use]
    pub fn rule_options(&self) -> BTreeMap<String, BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        for (code, setting) in &self.rules {
            let mut opts = BTreeMap::new();
            for (key, value) in &setting.options {
                if let Some(s) = toml_value_to_string(value) {
                    opts.insert(key.clone(), s);
                }
            }
            if !opts.is_empty() {
                out.insert(code.clone(), opts);
            }
        }
        out
    }
}

/// Renders a TOML option value as the flat string the analyzer consumes.
fn toml_value_to_string(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Integer(i) => Some(i.to_string()),
        toml::Value::Boolean(b) => Some(b.to_string()),
        toml::Value::Float(f) => Some(f.to_string()),
        toml::Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(toml_value_to_string).collect();
            Some(parts.join(","))
        }
        toml::Value::Datetime(_) | toml::Value::Table(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_enables_all() {
        let s = LintSettings::default();
        assert!(s.is_rule_enabled("RF01"));
    }

    #[test]
    fn master_switch_off() {
        let s = LintSettings {
            enabled: false,
            ..Default::default()
        };
        assert!(!s.is_rule_enabled("RF01"));
    }

    #[test]
    fn select_restricts() {
        let s = LintSettings {
            select: vec!["RF01".into()],
            ..Default::default()
        };
        assert!(s.is_rule_enabled("RF01"));
        assert!(!s.is_rule_enabled("CP01"));
    }

    #[test]
    fn per_rule_enabled_wins_over_exclude() {
        let mut rules = BTreeMap::new();
        rules.insert(
            "RF01".to_string(),
            RuleSetting {
                enabled: Some(true),
                ..Default::default()
            },
        );
        let s = LintSettings {
            exclude: vec!["RF01".into()],
            rules,
            ..Default::default()
        };
        assert!(s.is_rule_enabled("RF01"));
    }
}

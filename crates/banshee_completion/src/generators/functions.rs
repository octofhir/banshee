//! Function completion generator.

mod builtins;

use crate::providers::FunctionProvider;
use crate::types::{
    CompletionData, CompletionItem, CompletionItemKind, FunctionInfo, display_identifier,
};

/// Default search path for schema-qualified display decisions.
const DEFAULT_SEARCH_PATH: &[&str] = &["pg_catalog", "public"];

/// Generates function completion items.
pub fn complete_functions(
    provider: Option<&dyn FunctionProvider>,
    prefix: Option<&str>,
) -> Vec<CompletionItem> {
    complete_functions_with_options(provider, prefix, DEFAULT_SEARCH_PATH)
}

/// Generates function completion items with custom search path.
pub fn complete_functions_with_options(
    provider: Option<&dyn FunctionProvider>,
    prefix: Option<&str>,
    search_path: &[&str],
) -> Vec<CompletionItem> {
    let Some(provider) = provider else {
        return builtin_functions(prefix);
    };

    let functions = match prefix {
        Some(p) if !p.is_empty() => {
            // Try fuzzy matching
            let all_functions = provider.functions();
            filter_functions_fuzzy(&all_functions, p)
        }
        Some(p) => provider.functions_by_prefix(p),
        None => provider.functions(),
    };

    let mut items: Vec<_> = functions
        .into_iter()
        .map(|f| create_function_item_with_options(f, search_path))
        .collect();

    // Also include built-in functions
    items.extend(builtin_functions(prefix));

    // Sort by relevance (prefix matches first, then fuzzy matches)
    let prefix_lower = prefix.map(|p| p.to_lowercase());
    items.sort_by(|a, b| {
        let a_relevance = compute_relevance(&a.label, prefix_lower.as_deref());
        let b_relevance = compute_relevance(&b.label, prefix_lower.as_deref());
        a_relevance.cmp(&b_relevance).then(a.label.cmp(&b.label))
    });

    // Deduplicate by label
    items.dedup_by(|a, b| a.label.to_lowercase() == b.label.to_lowercase());

    items
}

/// Filters functions using fuzzy matching.
fn filter_functions_fuzzy(functions: &[FunctionInfo], prefix: &str) -> Vec<FunctionInfo> {
    let prefix_lower = prefix.to_lowercase();

    functions
        .iter()
        .filter(|f| {
            let name_lower = f.name.to_lowercase();
            // Prefix match
            name_lower.starts_with(&prefix_lower)
                // Contains match
                || name_lower.contains(&prefix_lower)
                // Abbreviation match (e.g., "jep" matches "jsonb_extract_path")
                || matches_abbreviation(&name_lower, &prefix_lower)
        })
        .cloned()
        .collect()
}

/// Checks if the pattern matches the name as an abbreviation.
///
/// Examples:
/// - `jep` matches `jsonb_extract_path`
/// - `jba` matches `jsonb_build_array`
/// - `cb` matches `concat_ws`
pub fn matches_abbreviation(name: &str, pattern: &str) -> bool {
    if pattern.is_empty() || name.is_empty() {
        return false;
    }

    let mut pattern_chars = pattern.chars().peekable();
    let mut current_pattern = pattern_chars.next();
    let mut at_word_start = true;

    for c in name.chars() {
        let Some(pattern_char) = current_pattern else {
            // Pattern fully matched
            return true;
        };

        // Match at word boundaries (start of name, after underscore)
        if at_word_start && c.eq_ignore_ascii_case(&pattern_char) {
            current_pattern = pattern_chars.next();
        }

        at_word_start = c == '_';
    }

    // Pattern is fully matched if we consumed all pattern chars
    current_pattern.is_none()
}

/// Computes relevance score for sorting (lower is better).
fn compute_relevance(name: &str, prefix: Option<&str>) -> u32 {
    let Some(prefix) = prefix else {
        return if is_common_function(name) { 0 } else { 1 };
    };

    let name_lower = name.to_lowercase();
    let prefix_lower = prefix.to_lowercase();

    // Common function bonus
    let base = if is_common_function(name) { 0 } else { 100 };

    if name_lower.starts_with(&prefix_lower) {
        // Exact prefix match - best
        base
    } else if name_lower.contains(&prefix_lower) {
        // Contains match - good
        base + 10
    } else if matches_abbreviation(&name_lower, &prefix_lower) {
        // Abbreviation match - acceptable
        base + 20
    } else {
        // No match
        base + 50
    }
}

/// Creates a completion item for a function with custom search path.
///
/// If the function's schema is not in the search path, the label will be schema-qualified.
fn create_function_item_with_options(func: FunctionInfo, search_path: &[&str]) -> CompletionItem {
    let signature = func.signature();

    // Determine if we need to qualify with schema
    let name = display_identifier(&func.name);
    let label = if let Some(schema) = func
        .schema
        .as_deref()
        .filter(|schema| !search_path.contains(schema))
    {
        format!("{}.{name}", display_identifier(schema))
    } else {
        name.into_owned()
    };

    let insert_text = format!("{}(", label);

    // Sort key: frequently used functions first
    let sort_key = if is_common_function(&func.name) {
        format!("0_{}", func.name.to_lowercase())
    } else {
        format!("1_{}", func.name.to_lowercase())
    };

    CompletionItem::new(CompletionItemKind::Function, &label)
        .with_detail(signature.clone())
        .with_insert_text(insert_text)
        .with_sort_key(sort_key)
        .with_filter_text(func.name.clone()) // Always filter on just the function name
        .with_data(CompletionData::Function {
            schema: func.schema.clone(),
            name: func.name.clone(),
            signature: Some(signature),
        })
        .with_documentation(func.description.clone().unwrap_or_default())
}

/// Returns true if this is a commonly used function.
fn is_common_function(name: &str) -> bool {
    let common = [
        "count",
        "sum",
        "avg",
        "min",
        "max",
        "coalesce",
        "nullif",
        "now",
        "current_timestamp",
        "extract",
        "cast",
        "to_char",
        "to_date",
        "to_timestamp",
        "array_agg",
        "string_agg",
        "jsonb_build_object",
        "jsonb_agg",
        "row_number",
        "rank",
        "dense_rank",
        "lag",
        "lead",
        "first_value",
        "last_value",
    ];
    common.iter().any(|c| c.eq_ignore_ascii_case(name))
}

/// Returns built-in SQL functions.
fn builtin_functions(prefix: Option<&str>) -> Vec<CompletionItem> {
    let prefix_lower = prefix.map(str::to_ascii_lowercase);
    builtins::FUNCTIONS
        .iter()
        .filter(|function| {
            prefix_lower
                .as_ref()
                .is_none_or(|p| function.name.starts_with(p))
        })
        .map(|function| {
            let name = function.name;
            let signature = function.signature();
            let sort_key = if is_common_function(name) {
                format!("0_{name}")
            } else {
                format!("1_{name}")
            };

            CompletionItem::new(CompletionItemKind::Function, name)
                .with_detail(signature.clone())
                .with_insert_text(function.insertion())
                .with_sort_key(sort_key)
                .with_documentation(function.description.to_string())
                .with_data(CompletionData::Function {
                    schema: None,
                    name: name.to_string(),
                    signature: Some(signature),
                })
        })
        .collect()
}

/// Returns function signature help for a specific function.
pub fn get_function_signature(
    provider: Option<&dyn FunctionProvider>,
    name: &str,
) -> Option<FunctionInfo> {
    if let Some(provider) = provider
        && let Some(func) = provider.function(None, name)
    {
        return Some(func);
    }

    builtins::FUNCTIONS
        .iter()
        .find(|function| function.name.eq_ignore_ascii_case(name))?
        .info()
}

/// Describes what kind of completion to provide for a JSONB function argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonbArgCompletion {
    /// Suggest JSONB columns from tables in scope
    JsonbColumn,
    /// Suggest field names as path keys (for jsonb_extract_path, etc.)
    PathKeys,
    /// Trigger JSONPath completion (for jsonb_path_* functions)
    JsonPath,
    /// No special completion (e.g., value argument in jsonb_set)
    None,
}

/// Returns the type of completion to provide for a JSONB function argument.
///
/// Returns `None` if the function is not a JSONB function.
pub fn get_jsonb_arg_completion(function: &str, arg_index: usize) -> Option<JsonbArgCompletion> {
    let func_lower = function.to_lowercase();

    match func_lower.as_str() {
        // Functions with VARIADIC path keys
        "jsonb_extract_path" | "jsonb_extract_path_text" => match arg_index {
            0 => Some(JsonbArgCompletion::JsonbColumn),
            _ => Some(JsonbArgCompletion::PathKeys),
        },
        // Functions with JSONPath argument
        "jsonb_path_exists"
        | "jsonb_path_match"
        | "jsonb_path_query"
        | "jsonb_path_query_array"
        | "jsonb_path_query_first" => match arg_index {
            0 => Some(JsonbArgCompletion::JsonbColumn),
            1 => Some(JsonbArgCompletion::JsonPath),
            _ => Some(JsonbArgCompletion::None),
        },
        // Functions with text[] path argument
        "jsonb_set" | "jsonb_insert" => match arg_index {
            0 => Some(JsonbArgCompletion::JsonbColumn),
            1 => Some(JsonbArgCompletion::PathKeys),
            _ => Some(JsonbArgCompletion::None),
        },
        "jsonb_delete_path" => match arg_index {
            0 => Some(JsonbArgCompletion::JsonbColumn),
            1 => Some(JsonbArgCompletion::PathKeys),
            _ => Some(JsonbArgCompletion::None),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::MemoryFunctionProvider;

    #[test]
    fn test_complete_functions_builtin() {
        let items = complete_functions(None, None);
        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label == "count"));
        assert!(items.iter().any(|i| i.label == "jsonb_build_object"));
    }

    #[test]
    fn test_complete_functions_with_prefix() {
        let items = complete_functions(None, Some("json"));
        assert!(!items.is_empty());
        assert!(items.iter().all(|i| i.label.starts_with("json")));
    }

    #[test]
    fn test_complete_functions_with_provider() {
        let provider = MemoryFunctionProvider::new()
            .add_function(FunctionInfo::new("my_custom_func", "integer"));

        let items = complete_functions(Some(&provider), None);
        assert!(items.iter().any(|i| i.label == "my_custom_func"));
        // Should also include built-ins
        assert!(items.iter().any(|i| i.label == "count"));
    }

    #[test]
    fn test_get_function_signature() {
        let info = get_function_signature(None, "count");
        assert!(info.is_some());
        let func = info.unwrap();
        assert_eq!(func.name, "count");
        assert!(!func.args.is_empty());
    }

    #[test]
    fn builtin_completion_and_signature_share_function_metadata() {
        let items = complete_functions(None, None);
        for name in [
            "lower",
            "upper",
            "concat_ws",
            "jsonb_extract_path",
            "coalesce",
        ] {
            let item = items.iter().find(|item| item.label == name).unwrap();
            let info = get_function_signature(None, &name.to_ascii_uppercase()).unwrap();
            assert_eq!(item.detail.as_deref(), Some(info.signature().as_str()));
        }
        let current_date = items
            .iter()
            .find(|item| item.label == "current_date")
            .unwrap();
        assert_eq!(current_date.insert_text.as_deref(), Some("current_date"));
    }

    #[test]
    fn test_matches_abbreviation() {
        // Positive cases
        assert!(matches_abbreviation("jsonb_extract_path", "jep"));
        assert!(matches_abbreviation("jsonb_build_array", "jba"));
        assert!(matches_abbreviation("jsonb_build_object", "jbo"));
        assert!(matches_abbreviation("array_agg", "aa"));
        assert!(matches_abbreviation("string_agg", "sa"));
        assert!(matches_abbreviation("row_number", "rn"));
        assert!(matches_abbreviation("dense_rank", "dr"));

        // Case insensitive
        assert!(matches_abbreviation("jsonb_extract_path", "JEP"));
        assert!(matches_abbreviation("JSONB_EXTRACT_PATH", "jep"));

        // Negative cases
        assert!(!matches_abbreviation("count", "xyz"));
        assert!(!matches_abbreviation("jsonb_extract_path", "jxp"));
        assert!(!matches_abbreviation("", "a"));
        assert!(!matches_abbreviation("count", ""));
    }

    #[test]
    fn test_fuzzy_matching() {
        let functions = vec![
            FunctionInfo::new("jsonb_extract_path", "jsonb"),
            FunctionInfo::new("jsonb_build_array", "jsonb"),
            FunctionInfo::new("jsonb_build_object", "jsonb"),
            FunctionInfo::new("count", "bigint"),
        ];

        // Prefix match
        let results = filter_functions_fuzzy(&functions, "jsonb");
        assert_eq!(results.len(), 3);

        // Contains match
        let results = filter_functions_fuzzy(&functions, "build");
        assert_eq!(results.len(), 2);

        // Abbreviation match
        let results = filter_functions_fuzzy(&functions, "jep");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "jsonb_extract_path");

        // No match
        let results = filter_functions_fuzzy(&functions, "xyz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_schema_qualified_display() {
        // Function in search path - should not be qualified
        let func_in_path = FunctionInfo::new("my_func", "integer").with_schema("public");
        let item = create_function_item_with_options(func_in_path, &["public", "pg_catalog"]);
        assert_eq!(item.label, "my_func");

        // Function not in search path - should be qualified
        let func_not_in_path = FunctionInfo::new("special_func", "text").with_schema("my_schema");
        let item = create_function_item_with_options(func_not_in_path, &["public", "pg_catalog"]);
        assert_eq!(item.label, "my_schema.special_func");

        // Filter text should still be just the function name
        assert_eq!(item.filter_text.as_deref(), Some("special_func"));
    }

    #[test]
    fn test_compute_relevance() {
        // Common function prefix match - best
        assert_eq!(compute_relevance("count", Some("cou")), 0);

        // Common function contains match
        assert_eq!(compute_relevance("count", Some("oun")), 10);

        // jsonb_build_object is in common list, so base is 0
        assert_eq!(compute_relevance("jsonb_build_object", Some("jsonb")), 0);

        // Non-common function abbreviation match
        // jsonb_extract_path_text is not in common list, so base is 100
        assert_eq!(compute_relevance("my_custom_func", Some("mcf")), 120);
    }

    // JSONB function argument completion tests

    #[test]
    fn test_jsonb_arg_completion_extract_path() {
        // First arg: JSONB column
        let result = get_jsonb_arg_completion("jsonb_extract_path", 0);
        assert_eq!(result, Some(JsonbArgCompletion::JsonbColumn));

        // Second arg onwards: path keys (variadic)
        let result = get_jsonb_arg_completion("jsonb_extract_path", 1);
        assert_eq!(result, Some(JsonbArgCompletion::PathKeys));

        let result = get_jsonb_arg_completion("jsonb_extract_path", 2);
        assert_eq!(result, Some(JsonbArgCompletion::PathKeys));

        let result = get_jsonb_arg_completion("jsonb_extract_path", 5);
        assert_eq!(result, Some(JsonbArgCompletion::PathKeys));
    }

    #[test]
    fn test_jsonb_arg_completion_extract_path_text() {
        let result = get_jsonb_arg_completion("jsonb_extract_path_text", 0);
        assert_eq!(result, Some(JsonbArgCompletion::JsonbColumn));

        let result = get_jsonb_arg_completion("jsonb_extract_path_text", 1);
        assert_eq!(result, Some(JsonbArgCompletion::PathKeys));
    }

    #[test]
    fn test_jsonb_arg_completion_path_query() {
        // First arg: JSONB column
        let result = get_jsonb_arg_completion("jsonb_path_query", 0);
        assert_eq!(result, Some(JsonbArgCompletion::JsonbColumn));

        // Second arg: JSONPath
        let result = get_jsonb_arg_completion("jsonb_path_query", 1);
        assert_eq!(result, Some(JsonbArgCompletion::JsonPath));

        // Third arg: no special completion
        let result = get_jsonb_arg_completion("jsonb_path_query", 2);
        assert_eq!(result, Some(JsonbArgCompletion::None));
    }

    #[test]
    fn test_jsonb_arg_completion_path_exists() {
        let result = get_jsonb_arg_completion("jsonb_path_exists", 0);
        assert_eq!(result, Some(JsonbArgCompletion::JsonbColumn));

        let result = get_jsonb_arg_completion("jsonb_path_exists", 1);
        assert_eq!(result, Some(JsonbArgCompletion::JsonPath));
    }

    #[test]
    fn test_jsonb_arg_completion_path_match() {
        let result = get_jsonb_arg_completion("jsonb_path_match", 0);
        assert_eq!(result, Some(JsonbArgCompletion::JsonbColumn));

        let result = get_jsonb_arg_completion("jsonb_path_match", 1);
        assert_eq!(result, Some(JsonbArgCompletion::JsonPath));
    }

    #[test]
    fn test_jsonb_arg_completion_set() {
        // First arg: JSONB column
        let result = get_jsonb_arg_completion("jsonb_set", 0);
        assert_eq!(result, Some(JsonbArgCompletion::JsonbColumn));

        // Second arg: path array
        let result = get_jsonb_arg_completion("jsonb_set", 1);
        assert_eq!(result, Some(JsonbArgCompletion::PathKeys));

        // Third arg: new value, no special completion
        let result = get_jsonb_arg_completion("jsonb_set", 2);
        assert_eq!(result, Some(JsonbArgCompletion::None));
    }

    #[test]
    fn test_jsonb_arg_completion_insert() {
        let result = get_jsonb_arg_completion("jsonb_insert", 0);
        assert_eq!(result, Some(JsonbArgCompletion::JsonbColumn));

        let result = get_jsonb_arg_completion("jsonb_insert", 1);
        assert_eq!(result, Some(JsonbArgCompletion::PathKeys));

        let result = get_jsonb_arg_completion("jsonb_insert", 2);
        assert_eq!(result, Some(JsonbArgCompletion::None));
    }

    #[test]
    fn test_jsonb_arg_completion_delete_path() {
        let result = get_jsonb_arg_completion("jsonb_delete_path", 0);
        assert_eq!(result, Some(JsonbArgCompletion::JsonbColumn));

        let result = get_jsonb_arg_completion("jsonb_delete_path", 1);
        assert_eq!(result, Some(JsonbArgCompletion::PathKeys));
    }

    #[test]
    fn test_jsonb_arg_completion_case_insensitive() {
        // Should work case-insensitively
        let result = get_jsonb_arg_completion("JSONB_EXTRACT_PATH", 0);
        assert_eq!(result, Some(JsonbArgCompletion::JsonbColumn));

        let result = get_jsonb_arg_completion("Jsonb_Path_Query", 1);
        assert_eq!(result, Some(JsonbArgCompletion::JsonPath));
    }

    #[test]
    fn test_jsonb_arg_completion_non_jsonb_function() {
        // Non-JSONB functions should return None
        let result = get_jsonb_arg_completion("count", 0);
        assert_eq!(result, None);

        let result = get_jsonb_arg_completion("sum", 0);
        assert_eq!(result, None);

        let result = get_jsonb_arg_completion("coalesce", 0);
        assert_eq!(result, None);
    }

    #[test]
    fn test_builtin_function_info_jsonb_path_query() {
        let info = get_function_signature(None, "jsonb_path_query");
        assert!(info.is_some());
        let func = info.unwrap();
        assert_eq!(func.name, "jsonb_path_query");
        assert_eq!(func.args.len(), 2);
        assert_eq!(func.args[0].data_type, "jsonb");
        assert_eq!(func.args[1].data_type, "jsonpath");
    }

    #[test]
    fn test_builtin_function_info_jsonb_set() {
        let info = get_function_signature(None, "jsonb_set");
        assert!(info.is_some());
        let func = info.unwrap();
        assert_eq!(func.name, "jsonb_set");
        assert_eq!(func.args.len(), 3);
        assert_eq!(func.args[0].data_type, "jsonb");
        assert_eq!(func.args[1].data_type, "text[]");
        assert_eq!(func.args[2].data_type, "jsonb");
    }
}

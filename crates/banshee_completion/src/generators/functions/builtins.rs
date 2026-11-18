//! Shared metadata for offline function completion and signature help.

use crate::types::{ArgMode, FunctionArg, FunctionInfo};

struct BuiltinArg {
    name: Option<&'static str>,
    data_type: &'static str,
    mode: ArgMode,
}

impl BuiltinArg {
    fn info(&self) -> FunctionArg {
        let mut arg = FunctionArg::new(self.data_type).with_mode(self.mode);
        arg.name = self.name.map(str::to_owned);
        arg
    }
}

const fn arg(data_type: &'static str) -> BuiltinArg {
    BuiltinArg {
        name: None,
        data_type,
        mode: ArgMode::In,
    }
}

const fn named(name: &'static str, data_type: &'static str) -> BuiltinArg {
    BuiltinArg {
        name: Some(name),
        ..arg(data_type)
    }
}

const fn variadic(name: Option<&'static str>, data_type: &'static str) -> BuiltinArg {
    BuiltinArg {
        name,
        data_type,
        mode: ArgMode::Variadic,
    }
}

enum Parameters {
    Call(&'static [BuiltinArg]),
    Repeated(&'static str),
    Syntax(&'static str),
}

use Parameters::{Call, Repeated, Syntax};

pub(super) struct Builtin {
    pub(super) name: &'static str,
    parameters: Parameters,
    return_type: &'static str,
    pub(super) description: &'static str,
}

const fn builtin(
    name: &'static str,
    parameters: Parameters,
    return_type: &'static str,
    description: &'static str,
) -> Builtin {
    Builtin {
        name,
        parameters,
        return_type,
        description,
    }
}

impl Builtin {
    pub(super) fn info(&self) -> Option<FunctionInfo> {
        let args = match self.parameters {
            Call(args) => args.iter().map(BuiltinArg::info).collect(),
            Repeated(value) => vec![FunctionArg::new(value), FunctionArg::new("...")],
            Syntax(_) => return None,
        };
        Some(self.function_info(args))
    }

    fn function_info(&self, args: Vec<FunctionArg>) -> FunctionInfo {
        FunctionInfo::new(self.name, self.return_type)
            .with_args(args)
            .with_description(self.description)
            .with_returns_set(self.return_type.starts_with("setof "))
    }

    pub(super) fn signature(&self) -> String {
        match self.parameters {
            Call(args) => self
                .function_info(args.iter().map(BuiltinArg::info).collect())
                .signature(),
            Repeated(value) => format!("{}({value}, ...) -> {}", self.name, self.return_type),
            Syntax(suffix) => format!("{}{suffix} -> {}", self.name, self.return_type),
        }
    }

    pub(super) fn insertion(&self) -> String {
        match self.parameters {
            Syntax("") => self.name.to_owned(),
            _ => format!("{}(", self.name),
        }
    }
}

pub(super) const FUNCTIONS: &[Builtin] = &[
    // Aggregate functions
    builtin("count", Call(&[arg("expression")]), "bigint", "Count rows"),
    builtin(
        "sum",
        Call(&[arg("expression")]),
        "numeric",
        "Sum of values",
    ),
    builtin(
        "avg",
        Call(&[arg("expression")]),
        "numeric",
        "Average of values",
    ),
    builtin(
        "min",
        Call(&[arg("expression")]),
        "same as input",
        "Minimum value",
    ),
    builtin(
        "max",
        Call(&[arg("expression")]),
        "same as input",
        "Maximum value",
    ),
    builtin(
        "array_agg",
        Call(&[arg("expression")]),
        "array",
        "Aggregate into array",
    ),
    builtin(
        "string_agg",
        Call(&[arg("expression"), arg("delimiter")]),
        "text",
        "Aggregate into string",
    ),
    builtin(
        "bool_and",
        Call(&[arg("expression")]),
        "boolean",
        "Logical AND",
    ),
    builtin(
        "bool_or",
        Call(&[arg("expression")]),
        "boolean",
        "Logical OR",
    ),
    // JSON/JSONB functions
    builtin(
        "jsonb_build_object",
        Call(&[variadic(None, "\"any\"")]),
        "jsonb",
        "Build JSONB object",
    ),
    builtin(
        "jsonb_build_array",
        Call(&[variadic(None, "\"any\"")]),
        "jsonb",
        "Build JSONB array",
    ),
    builtin(
        "jsonb_agg",
        Call(&[arg("expression")]),
        "jsonb",
        "Aggregate into JSONB array",
    ),
    builtin(
        "jsonb_object_agg",
        Call(&[arg("key"), arg("value")]),
        "jsonb",
        "Aggregate into JSONB object",
    ),
    builtin(
        "jsonb_extract_path",
        Call(&[
            named("from_json", "jsonb"),
            variadic(Some("path_elems"), "text[]"),
        ]),
        "jsonb",
        "Extract JSONB path",
    ),
    builtin(
        "jsonb_extract_path_text",
        Call(&[
            named("from_json", "jsonb"),
            variadic(Some("path_elems"), "text[]"),
        ]),
        "text",
        "Extract JSONB path as text",
    ),
    builtin(
        "jsonb_set",
        Call(&[
            named("target", "jsonb"),
            named("path", "text[]"),
            named("new_value", "jsonb"),
        ]),
        "jsonb",
        "Set JSONB value at path",
    ),
    builtin(
        "jsonb_insert",
        Call(&[
            named("target", "jsonb"),
            named("path", "text[]"),
            named("new_value", "jsonb"),
        ]),
        "jsonb",
        "Insert into JSONB",
    ),
    builtin(
        "jsonb_pretty",
        Call(&[arg("jsonb")]),
        "text",
        "Pretty-print JSONB",
    ),
    builtin(
        "jsonb_typeof",
        Call(&[arg("jsonb")]),
        "text",
        "Return JSONB type",
    ),
    builtin(
        "jsonb_array_elements",
        Call(&[arg("jsonb")]),
        "setof jsonb",
        "Expand JSONB array",
    ),
    builtin(
        "jsonb_array_elements_text",
        Call(&[arg("jsonb")]),
        "setof text",
        "Expand JSONB array as text",
    ),
    builtin(
        "jsonb_object_keys",
        Call(&[arg("jsonb")]),
        "setof text",
        "Return JSONB object keys",
    ),
    builtin(
        "jsonb_each",
        Call(&[arg("jsonb")]),
        "setof (key, value)",
        "Expand JSONB to key-value pairs",
    ),
    builtin(
        "jsonb_each_text",
        Call(&[arg("jsonb")]),
        "setof (key, value)",
        "Expand JSONB to text key-value pairs",
    ),
    builtin(
        "jsonb_strip_nulls",
        Call(&[arg("jsonb")]),
        "jsonb",
        "Remove null fields",
    ),
    builtin(
        "to_jsonb",
        Call(&[arg("value")]),
        "jsonb",
        "Convert to JSONB",
    ),
    builtin(
        "jsonb_delete_path",
        Call(&[named("target", "jsonb"), named("path", "text[]")]),
        "jsonb",
        "Delete JSONB value at path",
    ),
    // PostgreSQL 12+ JSONPath functions
    builtin(
        "jsonb_path_exists",
        Call(&[named("target", "jsonb"), named("path", "jsonpath")]),
        "boolean",
        "Check if JSONPath returns any items",
    ),
    builtin(
        "jsonb_path_match",
        Call(&[named("target", "jsonb"), named("path", "jsonpath")]),
        "boolean",
        "Check JSONPath predicate result",
    ),
    builtin(
        "jsonb_path_query",
        Call(&[named("target", "jsonb"), named("path", "jsonpath")]),
        "setof jsonb",
        "Extract values matching JSONPath",
    ),
    builtin(
        "jsonb_path_query_array",
        Call(&[named("target", "jsonb"), named("path", "jsonpath")]),
        "jsonb",
        "Extract values as JSONB array",
    ),
    builtin(
        "jsonb_path_query_first",
        Call(&[named("target", "jsonb"), named("path", "jsonpath")]),
        "jsonb",
        "Extract first value matching JSONPath",
    ),
    // Window functions
    builtin("row_number", Call(&[]), "bigint", "Row number in partition"),
    builtin("rank", Call(&[]), "bigint", "Rank with gaps"),
    builtin("dense_rank", Call(&[]), "bigint", "Rank without gaps"),
    builtin(
        "ntile",
        Call(&[arg("n")]),
        "integer",
        "Divide into n buckets",
    ),
    builtin(
        "lag",
        Call(&[arg("value"), arg("offset"), arg("default")]),
        "same as input",
        "Previous row value",
    ),
    builtin(
        "lead",
        Call(&[arg("value"), arg("offset"), arg("default")]),
        "same as input",
        "Next row value",
    ),
    builtin(
        "first_value",
        Call(&[arg("value")]),
        "same as input",
        "First value in window",
    ),
    builtin(
        "last_value",
        Call(&[arg("value")]),
        "same as input",
        "Last value in window",
    ),
    builtin(
        "nth_value",
        Call(&[arg("value"), arg("n")]),
        "same as input",
        "Nth value in window",
    ),
    // String functions
    builtin(
        "concat",
        Call(&[variadic(None, "\"any\"")]),
        "text",
        "Concatenate strings",
    ),
    builtin(
        "concat_ws",
        Call(&[named("separator", "text"), variadic(None, "\"any\"")]),
        "text",
        "Concatenate with separator",
    ),
    builtin("length", Call(&[arg("text")]), "integer", "String length"),
    builtin(
        "lower",
        Call(&[arg("text")]),
        "text",
        "Convert to lowercase",
    ),
    builtin(
        "upper",
        Call(&[arg("text")]),
        "text",
        "Convert to uppercase",
    ),
    builtin("trim", Call(&[arg("text")]), "text", "Remove whitespace"),
    builtin(
        "ltrim",
        Call(&[arg("text")]),
        "text",
        "Remove leading whitespace",
    ),
    builtin(
        "rtrim",
        Call(&[arg("text")]),
        "text",
        "Remove trailing whitespace",
    ),
    builtin(
        "substring",
        Call(&[arg("text"), arg("start"), arg("length")]),
        "text",
        "Extract substring",
    ),
    builtin(
        "replace",
        Call(&[arg("text"), arg("from"), arg("to")]),
        "text",
        "Replace occurrences",
    ),
    builtin(
        "split_part",
        Call(&[arg("text"), arg("delimiter"), arg("n")]),
        "text",
        "Split and return part",
    ),
    builtin(
        "regexp_replace",
        Call(&[arg("text"), arg("pattern"), arg("replacement")]),
        "text",
        "Regex replace",
    ),
    builtin(
        "regexp_matches",
        Call(&[arg("text"), arg("pattern")]),
        "text[]",
        "Regex matches",
    ),
    // Date/time functions
    builtin(
        "now",
        Call(&[]),
        "timestamp with time zone",
        "Current timestamp",
    ),
    builtin(
        "current_timestamp",
        Syntax(""),
        "timestamp with time zone",
        "Current timestamp",
    ),
    builtin("current_date", Syntax(""), "date", "Current date"),
    builtin(
        "current_time",
        Syntax(""),
        "time with time zone",
        "Current time",
    ),
    builtin(
        "extract",
        Syntax("(field from source)"),
        "numeric",
        "Extract date/time field",
    ),
    builtin(
        "date_trunc",
        Call(&[arg("field"), arg("source")]),
        "timestamp",
        "Truncate to specified precision",
    ),
    builtin(
        "date_part",
        Call(&[arg("field"), arg("source")]),
        "double precision",
        "Extract date/time part",
    ),
    builtin(
        "age",
        Call(&[arg("timestamp"), arg("timestamp")]),
        "interval",
        "Calculate age",
    ),
    builtin(
        "to_char",
        Call(&[arg("timestamp"), arg("format")]),
        "text",
        "Format as string",
    ),
    builtin(
        "to_date",
        Call(&[arg("text"), arg("format")]),
        "date",
        "Parse date",
    ),
    builtin(
        "to_timestamp",
        Call(&[arg("text"), arg("format")]),
        "timestamp",
        "Parse timestamp",
    ),
    // Conditional functions
    builtin(
        "coalesce",
        Repeated("value"),
        "same as first non-null",
        "Return first non-null",
    ),
    builtin(
        "nullif",
        Call(&[arg("value1"), arg("value2")]),
        "same as input",
        "Return null if equal",
    ),
    builtin(
        "greatest",
        Repeated("value"),
        "same as input",
        "Return greatest value",
    ),
    builtin(
        "least",
        Repeated("value"),
        "same as input",
        "Return smallest value",
    ),
    // Math functions
    builtin("abs", Call(&[arg("numeric")]), "numeric", "Absolute value"),
    builtin("ceil", Call(&[arg("numeric")]), "numeric", "Round up"),
    builtin("floor", Call(&[arg("numeric")]), "numeric", "Round down"),
    builtin(
        "round",
        Call(&[arg("numeric"), arg("scale")]),
        "numeric",
        "Round to scale",
    ),
    builtin(
        "trunc",
        Call(&[arg("numeric"), arg("scale")]),
        "numeric",
        "Truncate to scale",
    ),
    builtin(
        "sqrt",
        Call(&[arg("numeric")]),
        "double precision",
        "Square root",
    ),
    builtin(
        "power",
        Call(&[arg("base"), arg("exponent")]),
        "double precision",
        "Raise to power",
    ),
    builtin(
        "mod",
        Call(&[arg("dividend"), arg("divisor")]),
        "numeric",
        "Modulo",
    ),
    builtin("random", Call(&[]), "double precision", "Random value 0-1"),
    // Array functions
    builtin(
        "array_length",
        Call(&[arg("array"), arg("dimension")]),
        "integer",
        "Array length",
    ),
    builtin(
        "array_append",
        Call(&[arg("array"), arg("element")]),
        "array",
        "Append to array",
    ),
    builtin(
        "array_prepend",
        Call(&[arg("element"), arg("array")]),
        "array",
        "Prepend to array",
    ),
    builtin(
        "array_cat",
        Call(&[arg("array"), arg("array")]),
        "array",
        "Concatenate arrays",
    ),
    builtin(
        "array_position",
        Call(&[arg("array"), arg("element")]),
        "integer",
        "Find element position",
    ),
    builtin(
        "array_remove",
        Call(&[arg("array"), arg("element")]),
        "array",
        "Remove element",
    ),
    builtin(
        "unnest",
        Call(&[arg("array")]),
        "setof element",
        "Expand array to rows",
    ),
    // Type conversion
    builtin("cast", Syntax("(value AS type)"), "type", "Convert type"),
];

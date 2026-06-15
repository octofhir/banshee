#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // ==========================================================================
    // Trivia (preserved for formatting)
    // ==========================================================================
    WHITESPACE = 0,
    NEWLINE,
    LINE_COMMENT,
    BLOCK_COMMENT,

    // ==========================================================================
    // Literals
    // ==========================================================================
    INTEGER,
    FLOAT,
    STRING,
    DOLLAR_STRING,
    BIT_STRING,
    HEX_STRING,
    IDENT,
    QUOTED_IDENT,
    PARAM,

    // ==========================================================================
    // JSONB Operators (PostgreSQL-specific, critical for this parser)
    // ==========================================================================
    ARROW,           // ->  Extract as JSON
    ARROW_TEXT,      // ->> Extract as text
    HASH_ARROW,      // #>  Path extract as JSON
    HASH_ARROW_TEXT, // #>> Path extract as text
    AT_GT,           // @>  Contains
    LT_AT,           // <@  Contained by
    QUESTION,        // ?   Key exists
    QUESTION_PIPE,   // ?|  Any key exists
    QUESTION_AMP,    // ?&  All keys exist
    HASH_MINUS,      // #-  Delete path
    AT_QUESTION,     // @?  JSONPath exists
    AT_AT,           // @@  JSONPath match

    // ==========================================================================
    // Standard Operators
    // ==========================================================================
    EQ,              // =
    NE,              // <> or !=
    LT,              // <
    LE,              // <=
    GT,              // >
    GE,              // >=
    PLUS,            // +
    MINUS,           // -
    STAR,            // *
    SLASH,           // /
    PERCENT,         // %
    CARET,           // ^
    PIPE_PIPE,       // ||  Concatenation
    TILDE,           // ~   Regex match
    TILDE_STAR,      // ~*  Case-insensitive regex
    BANG_TILDE,      // !~  Regex not match
    BANG_TILDE_STAR, // !~* Case-insensitive regex not match
    AMP,             // &   Bitwise AND
    PIPE,            // |   Bitwise OR
    HASH,            // #   Bitwise XOR
    SHL,             // <<  Bitwise shift left
    SHR,             // >>  Bitwise shift right
    FAT_ARROW,       // =>  Named argument
    SIMILAR_TO,      // SIMILAR TO (parsed as keyword sequence)
    LIKE,            // LIKE (parsed as keyword)

    // ==========================================================================
    // Punctuation
    // ==========================================================================
    SEMICOLON,    // ;
    COMMA,        // ,
    DOT,          // .
    COLON,        // :
    DOUBLE_COLON, // :: (PostgreSQL cast)
    L_PAREN,      // (
    R_PAREN,      // )
    L_BRACKET,    // [
    R_BRACKET,    // ]
    L_BRACE,      // {
    R_BRACE,      // }
    DOLLAR,       // $

    // ==========================================================================
    // Keywords (PostgreSQL, alphabetically sorted)
    // ==========================================================================
    ABS_KW,
    ABORT_KW,
    ACTION_KW,
    ADD_KW,
    AGGREGATE_KW,
    ALL_KW,
    ALTER_KW,
    ANALYSE_KW,
    ANALYZE_KW,
    AND_KW,
    ANY_KW,
    ARRAY_KW,
    AS_KW,
    ASC_KW,
    ASYMMETRIC_KW,
    AT_KW,
    AUTHORIZATION_KW,
    AVG_KW,
    BEGIN_KW,
    BETWEEN_KW,
    BIGINT_KW,
    BINARY_KW,
    BIT_KW,
    BOOLEAN_KW,
    BOTH_KW,
    BY_KW,
    CACHE_KW,
    CALL_KW,
    CASCADE_KW,
    CASE_KW,
    CAST_KW,
    CHAIN_KW,
    CHAR_KW,
    CHARACTER_KW,
    CHECK_KW,
    COALESCE_KW,
    COLLATE_KW,
    COLUMN_KW,
    COMMENT_KW,
    COMMIT_KW,
    CONCURRENTLY_KW,
    CONFLICT_KW,
    CONSTRAINT_KW,
    COPY_KW,
    COUNT_KW,
    CREATE_KW,
    CROSS_KW,
    CURRENT_KW,
    CURRENT_DATE_KW,
    CURRENT_TIME_KW,
    CURRENT_TIMESTAMP_KW,
    CURRENT_USER_KW,
    CURSOR_KW,
    CYCLE_KW,
    DATABASE_KW,
    DATE_KW,
    DECIMAL_KW,
    DEFAULT_KW,
    DEFERRABLE_KW,
    DEFERRED_KW,
    DELETE_KW,
    DESC_KW,
    DISTINCT_KW,
    DO_KW,
    DOUBLE_KW,
    DROP_KW,
    ELSE_KW,
    END_KW,
    ESCAPE_KW,
    EXCEPT_KW,
    EXCLUDE_KW,
    EXCLUDING_KW,
    EXISTS_KW,
    EXPLAIN_KW,
    EXTENSION_KW,
    EXTRACT_KW,
    FALSE_KW,
    FETCH_KW,
    FILTER_KW,
    FIRST_KW,
    FLOAT_KW,
    FOLLOWING_KW,
    FOR_KW,
    FOREIGN_KW,
    FROM_KW,
    FULL_KW,
    FUNCTION_KW,
    GENERATED_KW,
    GRANT_KW,
    GREATEST_KW,
    GROUP_KW,
    GROUPS_KW,
    HAVING_KW,
    IDENTITY_KW,
    IF_KW,
    ILIKE_KW,
    IMMEDIATE_KW,
    IN_KW,
    INCLUDING_KW,
    INCREMENT_KW,
    INDEX_KW,
    INHERITS_KW,
    INITIALLY_KW,
    INNER_KW,
    INOUT_KW,
    INSERT_KW,
    INT_KW,
    INTEGER_KW,
    INTERSECT_KW,
    INTERVAL_KW,
    INTO_KW,
    IS_KW,
    ISNULL_KW,
    JOIN_KW,
    JSON_KW,
    JSONB_KW,
    KEY_KW,
    LAST_KW,
    LATERAL_KW,
    LAX_KW,
    LEADING_KW,
    LEAST_KW,
    LEFT_KW,
    LIKE_KW,
    LIMIT_KW,
    LOCAL_KW,
    LOCALTIME_KW,
    LOCALTIMESTAMP_KW,
    LOCK_KW,
    LOCKED_KW,
    MATCH_KW,
    MATCHED_KW,
    MATERIALIZED_KW,
    MAX_KW,
    MAXVALUE_KW,
    MERGE_KW,
    MIN_KW,
    MINVALUE_KW,
    NATURAL_KW,
    NEXT_KW,
    NO_KW,
    NOT_KW,
    NOTHING_KW,
    NOTNULL_KW,
    NOWAIT_KW,
    NULL_KW,
    NULLIF_KW,
    NULLS_KW,
    NUMERIC_KW,
    OF_KW,
    OFFSET_KW,
    ON_KW,
    ONLY_KW,
    OPERATOR_KW,
    OR_KW,
    ORDER_KW,
    OTHERS_KW,
    OUT_KW,
    OUTER_KW,
    OVER_KW,
    OVERLAPS_KW,
    OVERLAY_KW,
    OWNED_KW,
    OWNER_KW,
    PARTITION_KW,
    PLACING_KW,
    POSITION_KW,
    PRECEDING_KW,
    PRECISION_KW,
    PRIMARY_KW,
    PROCEDURE_KW,
    RANGE_KW,
    REAL_KW,
    RECURSIVE_KW,
    REFERENCES_KW,
    REFRESH_KW,
    RELEASE_KW,
    RENAME_KW,
    REPLACE_KW,
    REPLICA_KW,
    RESET_KW,
    RESTART_KW,
    RESTRICT_KW,
    RETURNING_KW,
    RETURNS_KW,
    REVOKE_KW,
    RIGHT_KW,
    ROLLBACK_KW,
    ROW_KW,
    ROWS_KW,
    SAVEPOINT_KW,
    SCHEMA_KW,
    SELECT_KW,
    SEQUENCE_KW,
    SESSION_USER_KW,
    SET_KW,
    SETOF_KW,
    SHARE_KW,
    SHOW_KW,
    SIMILAR_KW,
    SKIP_KW,
    SMALLINT_KW,
    SOME_KW,
    START_KW,
    STORAGE_KW,
    STRICT_KW,
    SUBSTRING_KW,
    SUM_KW,
    SYMMETRIC_KW,
    TABLE_KW,
    TEMP_KW,
    TEMPORARY_KW,
    TEXT_KW,
    THEN_KW,
    TIES_KW,
    TIME_KW,
    TIMESTAMP_KW,
    TIMESTAMPTZ_KW,
    TIMETZ_KW,
    TO_KW,
    TRAILING_KW,
    TRANSACTION_KW,
    TREAT_KW,
    TRIGGER_KW,
    TRIM_KW,
    TRUE_KW,
    TRUNCATE_KW,
    TYPE_KW,
    UNBOUNDED_KW,
    UNION_KW,
    UNIQUE_KW,
    UNKNOWN_KW,
    UNLOGGED_KW,
    UPDATE_KW,
    USING_KW,
    VACUUM_KW,
    VALUES_KW,
    VARCHAR_KW,
    VARIADIC_KW,
    VARYING_KW,
    VIEW_KW,
    WHEN_KW,
    WHERE_KW,
    WINDOW_KW,
    WITH_KW,
    WITHIN_KW,
    WITHOUT_KW,
    WORK_KW,
    ZONE_KW,

    // ==========================================================================
    // Composite Nodes (CST structure)
    // ==========================================================================
    SOURCE_FILE,
    ERROR,

    // Statements
    SELECT_STMT,
    INSERT_STMT,
    UPDATE_STMT,
    DELETE_STMT,
    CREATE_TABLE_STMT,
    DROP_STMT,
    ALTER_STMT,
    ALTER_TABLE_ACTION,
    CREATE_INDEX_STMT,
    TRUNCATE_STMT,
    EXPLAIN_STMT,
    CREATE_VIEW_STMT,
    CREATE_SEQUENCE_STMT,
    CREATE_SCHEMA_STMT,
    CREATE_EXTENSION_STMT,
    COMMENT_STMT,
    TRANSACTION_STMT,
    SET_STMT,
    SHOW_STMT,
    RESET_STMT,
    CALL_STMT,
    DO_STMT,
    VACUUM_STMT,
    ANALYZE_STMT,
    COPY_STMT,
    GRANT_STMT,
    REVOKE_STMT,
    MERGE_STMT,
    CREATE_TYPE_STMT,
    CREATE_FUNCTION_STMT,
    CREATE_TRIGGER_STMT,
    CREATE_DOMAIN_STMT,
    ALTER_DOMAIN_STMT,
    REINDEX_STMT,
    CLUSTER_STMT,

    // SELECT clauses
    SELECT_CLAUSE,
    SELECT_ITEM,
    SELECT_ITEM_LIST,
    FROM_CLAUSE,
    WHERE_CLAUSE,
    GROUP_BY_CLAUSE,
    HAVING_CLAUSE,
    ORDER_BY_CLAUSE,
    ORDER_BY_ITEM,
    LIMIT_CLAUSE,
    OFFSET_CLAUSE,
    FETCH_CLAUSE,
    FOR_CLAUSE,

    // WITH clause (CTEs)
    WITH_CLAUSE,
    CTE,
    CTE_LIST,

    // Table references
    TABLE_REF,
    TABLE_NAME,
    ALIAS,
    JOIN_EXPR,
    JOIN_CONDITION,
    SUBQUERY,

    // Expressions
    EXPR,
    BINARY_EXPR,
    UNARY_EXPR,
    PAREN_EXPR,
    LITERAL,
    COLUMN_REF,
    QUALIFIED_NAME,
    CAST_EXPR,
    CASE_EXPR,
    CASE_WHEN,
    CASE_ELSE,
    BETWEEN_EXPR,
    IN_EXPR,
    LIKE_EXPR,
    IS_EXPR,
    EXISTS_EXPR,
    SUBQUERY_EXPR,
    ARRAY_EXPR,
    ROW_EXPR,
    COALESCE_EXPR,
    NULLIF_EXPR,
    GREATEST_EXPR,
    LEAST_EXPR,
    ANY_EXPR,          // expr op ANY (array/subquery)
    ALL_EXPR,          // expr op ALL (array/subquery)
    AT_TIME_ZONE_EXPR, // expr AT TIME ZONE zone
    COLLATE_EXPR,      // expr COLLATE collation
    ARRAY_SLICE_EXPR,  // expr[lower:upper]

    // JSONB access expressions
    JSONB_ACCESS_EXPR,
    JSONB_PATH_EXPR,
    JSONB_CONTAINS_EXPR,
    JSONB_EXISTS_EXPR,

    // Function call nodes
    FUNC_CALL,
    ARG_LIST,
    NAMED_ARG,
    ORDER_BY_ARG,
    WINDOW_SPEC,
    OVER_CLAUSE,
    PARTITION_BY,
    FRAME_CLAUSE,
    FRAME_BOUND,
    FRAME_EXCLUSION,
    FILTER_CLAUSE,

    // ==========================================================================
    // JSONPath tokens (for embedded JSONPath parsing)
    // ==========================================================================
    JP_DOLLAR,     // $ root
    JP_AT,         // @ current item
    JP_DOT,        // . member access
    JP_DOTDOT,     // .. recursive descent
    JP_STAR,       // * wildcard
    JP_QUESTION,   // ? filter start
    JP_L_BRACKET,  // [
    JP_R_BRACKET,  // ]
    JP_L_PAREN,    // (
    JP_R_PAREN,    // )
    JP_COMMA,      // ,
    JP_COLON,      // : (for negative index like last - 1)
    JP_EQ_EQ,      // ==
    JP_BANG_EQ,    // !=
    JP_LT,         // <
    JP_LE,         // <=
    JP_GT,         // >
    JP_GE,         // >=
    JP_AMP_AMP,    // &&
    JP_PIPE_PIPE,  // ||
    JP_BANG,       // !
    JP_PLUS,       // +
    JP_MINUS,      // -
    JP_IDENT,      // identifier
    JP_STRING_LIT, // quoted string "..."
    JP_NUMBER_LIT, // numeric literal
    JP_WHITESPACE, // whitespace in JSONPath

    // JSONPath keywords
    JP_STRICT_KW,     // strict
    JP_LAX_KW,        // lax
    JP_TRUE_KW,       // true
    JP_FALSE_KW,      // false
    JP_NULL_KW,       // null
    JP_LAST_KW,       // last
    JP_TO_KW,         // to
    JP_EXISTS_KW,     // exists
    JP_LIKE_REGEX_KW, // like_regex
    JP_STARTS_KW,     // starts
    JP_WITH_KW,       // with (for "starts with")
    JP_FLAG_KW,       // flag
    JP_IS_KW,         // is
    JP_UNKNOWN_KW,    // unknown
    JP_TYPE_KW,       // type()
    JP_SIZE_KW,       // size()
    JP_DOUBLE_KW,     // double()
    JP_CEILING_KW,    // ceiling()
    JP_FLOOR_KW,      // floor()
    JP_ABS_KW,        // abs()
    JP_KEYVALUE_KW,   // keyvalue()
    JP_DATETIME_KW,   // datetime()

    // ==========================================================================
    // JSONPath nodes (for @? and @@ operators)
    // ==========================================================================
    JSONPATH_LITERAL,
    JP_ROOT,
    JP_CURRENT,
    JP_MEMBER_ACCESS,
    JP_MEMBER_WILDCARD,
    JP_RECURSIVE_DESCENT,
    JP_ARRAY_ACCESS,
    JP_ARRAY_SLICE,
    JP_ARRAY_WILDCARD,
    JP_FILTER,
    JP_FILTER_EXPR,
    JP_COMPARISON,
    JP_LOGICAL_AND,
    JP_LOGICAL_OR,
    JP_LOGICAL_NOT,
    JP_EXISTS,
    JP_METHOD,
    JP_ARITHMETIC,
    JP_PAREN,
    JP_STRING,
    JP_NUMBER,
    JP_BOOL,
    JP_NULL,
    JP_VARIABLE,
    JP_MODE,
    JP_KEY,           // member key (wrapped identifier or string)
    JP_QUOTE,         // quote character (' or ") around JSONPath literal
    JSONPATH_CONTENT, // parsed content of a JSONPath literal

    // Column/table definitions
    COLUMN_DEF,
    COLUMN_DEF_LIST,
    TYPE_NAME,
    CONSTRAINT,
    CONSTRAINT_LIST,
    PRIMARY_KEY_CONSTRAINT,
    FOREIGN_KEY_CONSTRAINT,
    UNIQUE_CONSTRAINT,
    CHECK_CONSTRAINT,
    NOT_NULL_CONSTRAINT,
    DEFAULT_EXPR,

    // INSERT specifics
    INSERT_COLUMNS,
    VALUES_CLAUSE,
    VALUES_ROW,
    ON_CONFLICT_CLAUSE,
    CONFLICT_TARGET,
    CONFLICT_ACTION,
    DO_UPDATE,
    DO_NOTHING,

    // UPDATE specifics
    SET_CLAUSE,
    SET_ITEM,

    // DELETE specifics
    USING_CLAUSE,

    // RETURNING clause (shared by INSERT, UPDATE, DELETE)
    RETURNING_CLAUSE,

    // Type expressions
    ARRAY_TYPE,
    INTERVAL_FIELD,

    // Misc
    COMMENT,
    NAME,
    STAR_EXPR,
    PARAM_REF,

    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::WHITESPACE
                | SyntaxKind::NEWLINE
                | SyntaxKind::LINE_COMMENT
                | SyntaxKind::BLOCK_COMMENT
        )
    }

    pub fn is_jp_trivia(self) -> bool {
        matches!(self, SyntaxKind::JP_WHITESPACE)
    }

    pub fn is_keyword(self) -> bool {
        let v = self as u32;
        v >= SyntaxKind::ABS_KW as u32 && v <= SyntaxKind::ZONE_KW as u32
    }

    pub fn is_jsonb_operator(self) -> bool {
        matches!(
            self,
            SyntaxKind::ARROW
                | SyntaxKind::ARROW_TEXT
                | SyntaxKind::HASH_ARROW
                | SyntaxKind::HASH_ARROW_TEXT
                | SyntaxKind::AT_GT
                | SyntaxKind::LT_AT
                | SyntaxKind::QUESTION
                | SyntaxKind::QUESTION_PIPE
                | SyntaxKind::QUESTION_AMP
                | SyntaxKind::HASH_MINUS
                | SyntaxKind::AT_QUESTION
                | SyntaxKind::AT_AT
        )
    }

    pub fn is_literal(self) -> bool {
        matches!(
            self,
            SyntaxKind::INTEGER
                | SyntaxKind::FLOAT
                | SyntaxKind::STRING
                | SyntaxKind::DOLLAR_STRING
                | SyntaxKind::BIT_STRING
                | SyntaxKind::HEX_STRING
        )
    }
}

impl From<SyntaxKind> for u32 {
    fn from(kind: SyntaxKind) -> u32 {
        kind as u32
    }
}

impl From<u32> for SyntaxKind {
    fn from(raw: u32) -> SyntaxKind {
        assert!(raw < SyntaxKind::__LAST as u32, "invalid SyntaxKind: {raw}");
        unsafe { std::mem::transmute(raw) }
    }
}

impl cstree::Syntax for SyntaxKind {
    fn from_raw(raw: cstree::RawSyntaxKind) -> Self {
        Self::from(raw.0)
    }

    fn into_raw(self) -> cstree::RawSyntaxKind {
        cstree::RawSyntaxKind(self as u32)
    }

    fn static_text(self) -> Option<&'static str> {
        match self {
            // Punctuation with static text
            Self::SEMICOLON => Some(";"),
            Self::COMMA => Some(","),
            Self::DOT => Some("."),
            Self::COLON => Some(":"),
            Self::DOUBLE_COLON => Some("::"),
            Self::L_PAREN => Some("("),
            Self::R_PAREN => Some(")"),
            Self::L_BRACKET => Some("["),
            Self::R_BRACKET => Some("]"),
            Self::L_BRACE => Some("{"),
            Self::R_BRACE => Some("}"),
            Self::DOLLAR => Some("$"),
            // Operators
            Self::EQ => Some("="),
            // NE has two spellings (`<>` and `!=`); no single static text.
            Self::LT => Some("<"),
            Self::LE => Some("<="),
            Self::GT => Some(">"),
            Self::GE => Some(">="),
            Self::PLUS => Some("+"),
            Self::MINUS => Some("-"),
            Self::STAR => Some("*"),
            Self::SLASH => Some("/"),
            Self::PERCENT => Some("%"),
            Self::CARET => Some("^"),
            Self::PIPE_PIPE => Some("||"),
            Self::TILDE => Some("~"),
            Self::AMP => Some("&"),
            Self::PIPE => Some("|"),
            Self::HASH => Some("#"),
            Self::SHL => Some("<<"),
            Self::SHR => Some(">>"),
            Self::FAT_ARROW => Some("=>"),
            // JSONB operators
            Self::ARROW => Some("->"),
            Self::ARROW_TEXT => Some("->>"),
            Self::HASH_ARROW => Some("#>"),
            Self::HASH_ARROW_TEXT => Some("#>>"),
            Self::AT_GT => Some("@>"),
            Self::LT_AT => Some("<@"),
            Self::QUESTION => Some("?"),
            Self::QUESTION_PIPE => Some("?|"),
            Self::QUESTION_AMP => Some("?&"),
            Self::HASH_MINUS => Some("#-"),
            Self::AT_QUESTION => Some("@?"),
            Self::AT_AT => Some("@@"),
            _ => None,
        }
    }
}

pub fn keyword_from_str(s: &str) -> Option<SyntaxKind> {
    let upper = s.to_ascii_uppercase();
    KEYWORD_MAP.get(upper.as_str()).copied()
}

static KEYWORD_MAP: phf::Map<&'static str, SyntaxKind> = phf::phf_map! {
    "ABORT" => SyntaxKind::ABORT_KW,
    "ABS" => SyntaxKind::ABS_KW,
    "ACTION" => SyntaxKind::ACTION_KW,
    "ADD" => SyntaxKind::ADD_KW,
    "AGGREGATE" => SyntaxKind::AGGREGATE_KW,
    "ALL" => SyntaxKind::ALL_KW,
    "ALTER" => SyntaxKind::ALTER_KW,
    "ANALYSE" => SyntaxKind::ANALYSE_KW,
    "ANALYZE" => SyntaxKind::ANALYZE_KW,
    "AND" => SyntaxKind::AND_KW,
    "ANY" => SyntaxKind::ANY_KW,
    "ARRAY" => SyntaxKind::ARRAY_KW,
    "AS" => SyntaxKind::AS_KW,
    "ASC" => SyntaxKind::ASC_KW,
    "ASYMMETRIC" => SyntaxKind::ASYMMETRIC_KW,
    "AT" => SyntaxKind::AT_KW,
    "AUTHORIZATION" => SyntaxKind::AUTHORIZATION_KW,
    "AVG" => SyntaxKind::AVG_KW,
    "BEGIN" => SyntaxKind::BEGIN_KW,
    "BETWEEN" => SyntaxKind::BETWEEN_KW,
    "BIGINT" => SyntaxKind::BIGINT_KW,
    "BINARY" => SyntaxKind::BINARY_KW,
    "BIT" => SyntaxKind::BIT_KW,
    "BOOLEAN" => SyntaxKind::BOOLEAN_KW,
    "BOTH" => SyntaxKind::BOTH_KW,
    "BY" => SyntaxKind::BY_KW,
    "CACHE" => SyntaxKind::CACHE_KW,
    "CALL" => SyntaxKind::CALL_KW,
    "CASCADE" => SyntaxKind::CASCADE_KW,
    "CASE" => SyntaxKind::CASE_KW,
    "CAST" => SyntaxKind::CAST_KW,
    "CHAIN" => SyntaxKind::CHAIN_KW,
    "CHAR" => SyntaxKind::CHAR_KW,
    "CHARACTER" => SyntaxKind::CHARACTER_KW,
    "CHECK" => SyntaxKind::CHECK_KW,
    "COALESCE" => SyntaxKind::COALESCE_KW,
    "COLLATE" => SyntaxKind::COLLATE_KW,
    "COLUMN" => SyntaxKind::COLUMN_KW,
    "COMMENT" => SyntaxKind::COMMENT_KW,
    "COMMIT" => SyntaxKind::COMMIT_KW,
    "CONCURRENTLY" => SyntaxKind::CONCURRENTLY_KW,
    "CONFLICT" => SyntaxKind::CONFLICT_KW,
    "CONSTRAINT" => SyntaxKind::CONSTRAINT_KW,
    "COPY" => SyntaxKind::COPY_KW,
    "COUNT" => SyntaxKind::COUNT_KW,
    "CREATE" => SyntaxKind::CREATE_KW,
    "CROSS" => SyntaxKind::CROSS_KW,
    "CURRENT" => SyntaxKind::CURRENT_KW,
    "CURRENT_DATE" => SyntaxKind::CURRENT_DATE_KW,
    "CURRENT_TIME" => SyntaxKind::CURRENT_TIME_KW,
    "CURRENT_TIMESTAMP" => SyntaxKind::CURRENT_TIMESTAMP_KW,
    "CURRENT_USER" => SyntaxKind::CURRENT_USER_KW,
    "CURSOR" => SyntaxKind::CURSOR_KW,
    "CYCLE" => SyntaxKind::CYCLE_KW,
    "DATABASE" => SyntaxKind::DATABASE_KW,
    "DATE" => SyntaxKind::DATE_KW,
    "DECIMAL" => SyntaxKind::DECIMAL_KW,
    "DEFAULT" => SyntaxKind::DEFAULT_KW,
    "DEFERRABLE" => SyntaxKind::DEFERRABLE_KW,
    "DEFERRED" => SyntaxKind::DEFERRED_KW,
    "DELETE" => SyntaxKind::DELETE_KW,
    "DESC" => SyntaxKind::DESC_KW,
    "DISTINCT" => SyntaxKind::DISTINCT_KW,
    "DO" => SyntaxKind::DO_KW,
    "DOUBLE" => SyntaxKind::DOUBLE_KW,
    "DROP" => SyntaxKind::DROP_KW,
    "ELSE" => SyntaxKind::ELSE_KW,
    "END" => SyntaxKind::END_KW,
    "ESCAPE" => SyntaxKind::ESCAPE_KW,
    "EXCEPT" => SyntaxKind::EXCEPT_KW,
    "EXCLUDE" => SyntaxKind::EXCLUDE_KW,
    "EXCLUDING" => SyntaxKind::EXCLUDING_KW,
    "EXISTS" => SyntaxKind::EXISTS_KW,
    "EXPLAIN" => SyntaxKind::EXPLAIN_KW,
    "EXTENSION" => SyntaxKind::EXTENSION_KW,
    "EXTRACT" => SyntaxKind::EXTRACT_KW,
    "FALSE" => SyntaxKind::FALSE_KW,
    "FETCH" => SyntaxKind::FETCH_KW,
    "FILTER" => SyntaxKind::FILTER_KW,
    "FIRST" => SyntaxKind::FIRST_KW,
    "FLOAT" => SyntaxKind::FLOAT_KW,
    "FOLLOWING" => SyntaxKind::FOLLOWING_KW,
    "FOR" => SyntaxKind::FOR_KW,
    "FOREIGN" => SyntaxKind::FOREIGN_KW,
    "FROM" => SyntaxKind::FROM_KW,
    "FULL" => SyntaxKind::FULL_KW,
    "FUNCTION" => SyntaxKind::FUNCTION_KW,
    "GENERATED" => SyntaxKind::GENERATED_KW,
    "GRANT" => SyntaxKind::GRANT_KW,
    "GREATEST" => SyntaxKind::GREATEST_KW,
    "GROUP" => SyntaxKind::GROUP_KW,
    "GROUPS" => SyntaxKind::GROUPS_KW,
    "HAVING" => SyntaxKind::HAVING_KW,
    "IDENTITY" => SyntaxKind::IDENTITY_KW,
    "IF" => SyntaxKind::IF_KW,
    "ILIKE" => SyntaxKind::ILIKE_KW,
    "IMMEDIATE" => SyntaxKind::IMMEDIATE_KW,
    "IN" => SyntaxKind::IN_KW,
    "INCLUDING" => SyntaxKind::INCLUDING_KW,
    "INCREMENT" => SyntaxKind::INCREMENT_KW,
    "INDEX" => SyntaxKind::INDEX_KW,
    "INHERITS" => SyntaxKind::INHERITS_KW,
    "INITIALLY" => SyntaxKind::INITIALLY_KW,
    "INNER" => SyntaxKind::INNER_KW,
    "INOUT" => SyntaxKind::INOUT_KW,
    "INSERT" => SyntaxKind::INSERT_KW,
    "INT" => SyntaxKind::INT_KW,
    "INTEGER" => SyntaxKind::INTEGER_KW,
    "INTERSECT" => SyntaxKind::INTERSECT_KW,
    "INTERVAL" => SyntaxKind::INTERVAL_KW,
    "INTO" => SyntaxKind::INTO_KW,
    "IS" => SyntaxKind::IS_KW,
    "ISNULL" => SyntaxKind::ISNULL_KW,
    "JOIN" => SyntaxKind::JOIN_KW,
    "JSON" => SyntaxKind::JSON_KW,
    "JSONB" => SyntaxKind::JSONB_KW,
    "KEY" => SyntaxKind::KEY_KW,
    "LAST" => SyntaxKind::LAST_KW,
    "LATERAL" => SyntaxKind::LATERAL_KW,
    "LAX" => SyntaxKind::LAX_KW,
    "LEADING" => SyntaxKind::LEADING_KW,
    "LEAST" => SyntaxKind::LEAST_KW,
    "LEFT" => SyntaxKind::LEFT_KW,
    "LIKE" => SyntaxKind::LIKE_KW,
    "LIMIT" => SyntaxKind::LIMIT_KW,
    "LOCAL" => SyntaxKind::LOCAL_KW,
    "LOCALTIME" => SyntaxKind::LOCALTIME_KW,
    "LOCALTIMESTAMP" => SyntaxKind::LOCALTIMESTAMP_KW,
    "LOCK" => SyntaxKind::LOCK_KW,
    "LOCKED" => SyntaxKind::LOCKED_KW,
    "MATCH" => SyntaxKind::MATCH_KW,
    "MATCHED" => SyntaxKind::MATCHED_KW,
    "MATERIALIZED" => SyntaxKind::MATERIALIZED_KW,
    "MAX" => SyntaxKind::MAX_KW,
    "MAXVALUE" => SyntaxKind::MAXVALUE_KW,
    "MERGE" => SyntaxKind::MERGE_KW,
    "MIN" => SyntaxKind::MIN_KW,
    "MINVALUE" => SyntaxKind::MINVALUE_KW,
    "NATURAL" => SyntaxKind::NATURAL_KW,
    "NEXT" => SyntaxKind::NEXT_KW,
    "NO" => SyntaxKind::NO_KW,
    "NOT" => SyntaxKind::NOT_KW,
    "NOTHING" => SyntaxKind::NOTHING_KW,
    "NOTNULL" => SyntaxKind::NOTNULL_KW,
    "NOWAIT" => SyntaxKind::NOWAIT_KW,
    "NULL" => SyntaxKind::NULL_KW,
    "NULLIF" => SyntaxKind::NULLIF_KW,
    "NULLS" => SyntaxKind::NULLS_KW,
    "NUMERIC" => SyntaxKind::NUMERIC_KW,
    "OF" => SyntaxKind::OF_KW,
    "OFFSET" => SyntaxKind::OFFSET_KW,
    "ON" => SyntaxKind::ON_KW,
    "ONLY" => SyntaxKind::ONLY_KW,
    "OPERATOR" => SyntaxKind::OPERATOR_KW,
    "OR" => SyntaxKind::OR_KW,
    "ORDER" => SyntaxKind::ORDER_KW,
    "OTHERS" => SyntaxKind::OTHERS_KW,
    "OUT" => SyntaxKind::OUT_KW,
    "OUTER" => SyntaxKind::OUTER_KW,
    "OVER" => SyntaxKind::OVER_KW,
    "OVERLAPS" => SyntaxKind::OVERLAPS_KW,
    "OVERLAY" => SyntaxKind::OVERLAY_KW,
    "OWNED" => SyntaxKind::OWNED_KW,
    "OWNER" => SyntaxKind::OWNER_KW,
    "PARTITION" => SyntaxKind::PARTITION_KW,
    "PLACING" => SyntaxKind::PLACING_KW,
    "POSITION" => SyntaxKind::POSITION_KW,
    "PRECEDING" => SyntaxKind::PRECEDING_KW,
    "PRECISION" => SyntaxKind::PRECISION_KW,
    "PRIMARY" => SyntaxKind::PRIMARY_KW,
    "PROCEDURE" => SyntaxKind::PROCEDURE_KW,
    "RANGE" => SyntaxKind::RANGE_KW,
    "REAL" => SyntaxKind::REAL_KW,
    "RECURSIVE" => SyntaxKind::RECURSIVE_KW,
    "REFERENCES" => SyntaxKind::REFERENCES_KW,
    "REFRESH" => SyntaxKind::REFRESH_KW,
    "RELEASE" => SyntaxKind::RELEASE_KW,
    "RENAME" => SyntaxKind::RENAME_KW,
    "REPLACE" => SyntaxKind::REPLACE_KW,
    "REPLICA" => SyntaxKind::REPLICA_KW,
    "RESET" => SyntaxKind::RESET_KW,
    "RESTART" => SyntaxKind::RESTART_KW,
    "RESTRICT" => SyntaxKind::RESTRICT_KW,
    "RETURNING" => SyntaxKind::RETURNING_KW,
    "RETURNS" => SyntaxKind::RETURNS_KW,
    "REVOKE" => SyntaxKind::REVOKE_KW,
    "RIGHT" => SyntaxKind::RIGHT_KW,
    "ROLLBACK" => SyntaxKind::ROLLBACK_KW,
    "ROW" => SyntaxKind::ROW_KW,
    "ROWS" => SyntaxKind::ROWS_KW,
    "SAVEPOINT" => SyntaxKind::SAVEPOINT_KW,
    "SCHEMA" => SyntaxKind::SCHEMA_KW,
    "SELECT" => SyntaxKind::SELECT_KW,
    "SEQUENCE" => SyntaxKind::SEQUENCE_KW,
    "SESSION_USER" => SyntaxKind::SESSION_USER_KW,
    "SET" => SyntaxKind::SET_KW,
    "SETOF" => SyntaxKind::SETOF_KW,
    "SHARE" => SyntaxKind::SHARE_KW,
    "SHOW" => SyntaxKind::SHOW_KW,
    "SIMILAR" => SyntaxKind::SIMILAR_KW,
    "SKIP" => SyntaxKind::SKIP_KW,
    "SMALLINT" => SyntaxKind::SMALLINT_KW,
    "SOME" => SyntaxKind::SOME_KW,
    "START" => SyntaxKind::START_KW,
    "STORAGE" => SyntaxKind::STORAGE_KW,
    "STRICT" => SyntaxKind::STRICT_KW,
    "SUBSTRING" => SyntaxKind::SUBSTRING_KW,
    "SUM" => SyntaxKind::SUM_KW,
    "SYMMETRIC" => SyntaxKind::SYMMETRIC_KW,
    "TABLE" => SyntaxKind::TABLE_KW,
    "TEMP" => SyntaxKind::TEMP_KW,
    "TEMPORARY" => SyntaxKind::TEMPORARY_KW,
    "TEXT" => SyntaxKind::TEXT_KW,
    "THEN" => SyntaxKind::THEN_KW,
    "TIES" => SyntaxKind::TIES_KW,
    "TIME" => SyntaxKind::TIME_KW,
    "TIMESTAMP" => SyntaxKind::TIMESTAMP_KW,
    "TIMESTAMPTZ" => SyntaxKind::TIMESTAMPTZ_KW,
    "TIMETZ" => SyntaxKind::TIMETZ_KW,
    "TO" => SyntaxKind::TO_KW,
    "TRAILING" => SyntaxKind::TRAILING_KW,
    "TRANSACTION" => SyntaxKind::TRANSACTION_KW,
    "TREAT" => SyntaxKind::TREAT_KW,
    "TRIGGER" => SyntaxKind::TRIGGER_KW,
    "TRIM" => SyntaxKind::TRIM_KW,
    "TRUE" => SyntaxKind::TRUE_KW,
    "TRUNCATE" => SyntaxKind::TRUNCATE_KW,
    "TYPE" => SyntaxKind::TYPE_KW,
    "UNBOUNDED" => SyntaxKind::UNBOUNDED_KW,
    "UNION" => SyntaxKind::UNION_KW,
    "UNIQUE" => SyntaxKind::UNIQUE_KW,
    "UNKNOWN" => SyntaxKind::UNKNOWN_KW,
    "UNLOGGED" => SyntaxKind::UNLOGGED_KW,
    "UPDATE" => SyntaxKind::UPDATE_KW,
    "USING" => SyntaxKind::USING_KW,
    "VACUUM" => SyntaxKind::VACUUM_KW,
    "VALUES" => SyntaxKind::VALUES_KW,
    "VARCHAR" => SyntaxKind::VARCHAR_KW,
    "VARIADIC" => SyntaxKind::VARIADIC_KW,
    "VARYING" => SyntaxKind::VARYING_KW,
    "VIEW" => SyntaxKind::VIEW_KW,
    "WHEN" => SyntaxKind::WHEN_KW,
    "WHERE" => SyntaxKind::WHERE_KW,
    "WINDOW" => SyntaxKind::WINDOW_KW,
    "WITH" => SyntaxKind::WITH_KW,
    "WITHIN" => SyntaxKind::WITHIN_KW,
    "WITHOUT" => SyntaxKind::WITHOUT_KW,
    "WORK" => SyntaxKind::WORK_KW,
    "ZONE" => SyntaxKind::ZONE_KW,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_kind_roundtrip() {
        for i in 0..SyntaxKind::__LAST as u32 {
            let kind = SyntaxKind::from(i);
            assert_eq!(u32::from(kind), i);
        }
    }

    #[test]
    fn keyword_lookup() {
        assert_eq!(keyword_from_str("SELECT"), Some(SyntaxKind::SELECT_KW));
        assert_eq!(keyword_from_str("select"), Some(SyntaxKind::SELECT_KW));
        assert_eq!(keyword_from_str("SeLeCt"), Some(SyntaxKind::SELECT_KW));
        assert_eq!(keyword_from_str("foobar"), None);
    }

    #[test]
    fn trivia_detection() {
        assert!(SyntaxKind::WHITESPACE.is_trivia());
        assert!(SyntaxKind::LINE_COMMENT.is_trivia());
        assert!(!SyntaxKind::SELECT_KW.is_trivia());
    }

    #[test]
    fn jsonb_operator_detection() {
        assert!(SyntaxKind::ARROW.is_jsonb_operator());
        assert!(SyntaxKind::AT_GT.is_jsonb_operator());
        assert!(!SyntaxKind::PLUS.is_jsonb_operator());
    }
}

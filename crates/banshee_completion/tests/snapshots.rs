use banshee_completion::{
    ColumnInfo, CompletionItem, CompletionRequest, CompletionResult, JsonbField, JsonbFieldType,
    JsonbSchema, MemorySchemaProvider, TableInfo, complete, complete_jsonb_paths,
};
use insta::assert_snapshot;
use text_size::TextSize;

fn test_provider() -> MemorySchemaProvider {
    MemorySchemaProvider::new()
        .add_table(TableInfo::new("users").with_schema("public"))
        .add_columns(
            Some("public".to_string()),
            "users",
            vec![
                ColumnInfo::new("id", "integer"),
                ColumnInfo::new("name", "text"),
                ColumnInfo::new("data", "jsonb"),
            ],
        )
        .add_table(TableInfo::new("orders").with_schema("public"))
        .add_columns(
            Some("public".to_string()),
            "orders",
            vec![
                ColumnInfo::new("id", "integer"),
                ColumnInfo::new("user_id", "integer"),
                ColumnInfo::new("amount", "numeric"),
            ],
        )
        .add_jsonb_schema(
            Some("public".to_string()),
            "users",
            "data",
            JsonbSchema::new()
                .with_field(JsonbField::new("email", JsonbFieldType::String))
                .with_field(JsonbField::new("preferences", JsonbFieldType::Object)),
        )
}

fn render_item(item: &CompletionItem) -> String {
    let insert = item.insert_text.as_deref().unwrap_or("-");
    let content = insert.trim_end_matches(' ');
    let trailing_spaces = "\\x20".repeat(insert.len() - content.len());
    format!(
        "{} | {:?} | detail={} | insert={content}{trailing_spaces}",
        item.label,
        item.kind,
        item.detail.as_deref().unwrap_or("-")
    )
}

fn render_completion(result: &CompletionResult) -> String {
    let mut lines = Vec::new();
    lines.push(format!("context: {:?}", result.context));
    lines.push(format!("is_incomplete: {}", result.is_incomplete));
    for item in &result.items {
        lines.push(render_item(item));
    }
    lines.join("\n")
}

fn render_items(items: &[CompletionItem]) -> String {
    items.iter().map(render_item).collect::<Vec<_>>().join("\n")
}

#[test]
fn snapshot_after_from() {
    let provider = test_provider();
    let source = "SELECT * FROM ";
    let result = complete(
        CompletionRequest::new(source, TextSize::new(source.len() as u32))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("after_from", render_completion(&result));
}

#[test]
fn snapshot_join_alias_columns() {
    let provider = test_provider();
    let source = "SELECT u. FROM users u JOIN orders o ON u.id = o.user_id";
    let offset = source.find(" FROM").unwrap() as u32;
    let result = complete(
        CompletionRequest::new(source, TextSize::new(offset))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("join_alias_columns", render_completion(&result));
}

#[test]
fn snapshot_jsonb_fields() {
    let provider = test_provider();
    let result = complete_jsonb_paths(Some(&provider), Some("public.users"), "data", &[]);

    assert_snapshot!("jsonb_fields", render_items(&result));
}

#[test]
fn snapshot_schema_qualified_columns() {
    let provider = test_provider();
    let source = "SELECT public.users. FROM public.users";
    let offset = source.find(" FROM").unwrap() as u32;
    let result = complete(
        CompletionRequest::new(source, TextSize::new(offset))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("schema_qualified_columns", render_completion(&result));
}

#[test]
fn snapshot_update_from_alias_columns() {
    let provider = test_provider();
    let source = "UPDATE users u SET name = o. FROM orders o WHERE u.id = o.user_id";
    let offset = source.find(" FROM").unwrap() as u32;
    let result = complete(
        CompletionRequest::new(source, TextSize::new(offset))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("update_from_alias_columns", render_completion(&result));
}

#[test]
fn snapshot_delete_using_alias_columns() {
    let provider = test_provider();
    let source = "DELETE FROM users u USING orders o WHERE o.";
    let result = complete(
        CompletionRequest::new(source, TextSize::new(source.len() as u32))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("delete_using_alias_columns", render_completion(&result));
}

#[test]
fn snapshot_insert_returning_alias_columns() {
    let provider = test_provider();
    let source = "INSERT INTO users u (name) VALUES ('alice') RETURNING u.";
    let result = complete(
        CompletionRequest::new(source, TextSize::new(source.len() as u32))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("insert_returning_alias_columns", render_completion(&result));
}

#[test]
fn snapshot_cte_columns() {
    let provider = test_provider();
    let source = "WITH recent AS (SELECT user_id, amount FROM orders) SELECT recent. FROM recent";
    let offset = source.find(" FROM recent").unwrap() as u32;
    let result = complete(
        CompletionRequest::new(source, TextSize::new(offset))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("cte_columns", render_completion(&result));
}

#[test]
fn snapshot_with_update_cte_columns() {
    let provider = test_provider();
    let source = "WITH recent AS (SELECT user_id, amount FROM orders) UPDATE users u SET name = recent. FROM recent WHERE u.id = recent.user_id";
    let offset = source.find(" FROM recent").unwrap() as u32;
    let result = complete(
        CompletionRequest::new(source, TextSize::new(offset))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("with_update_cte_columns", render_completion(&result));
}

#[test]
fn snapshot_with_delete_cte_columns() {
    let provider = test_provider();
    let source = "WITH recent AS (SELECT user_id, amount FROM orders) DELETE FROM users u USING recent WHERE recent.";
    let result = complete(
        CompletionRequest::new(source, TextSize::new(source.len() as u32))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("with_delete_cte_columns", render_completion(&result));
}

#[test]
fn snapshot_with_insert_source_cte_columns() {
    let provider = test_provider();
    let source = "WITH recent AS (SELECT user_id, amount FROM orders) INSERT INTO users (id, name) SELECT recent. FROM recent";
    let offset = source.find(" FROM recent").unwrap() as u32;
    let result = complete(
        CompletionRequest::new(source, TextSize::new(offset))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("with_insert_source_cte_columns", render_completion(&result));
}

#[test]
fn snapshot_dml_cte_columns() {
    let provider = test_provider();
    let source = "WITH changed AS (UPDATE orders SET amount = amount + 1 RETURNING user_id, amount) SELECT changed. FROM changed";
    let offset = source.find(" FROM changed").unwrap() as u32;
    let result = complete(
        CompletionRequest::new(source, TextSize::new(offset))
            .with_schema_provider(&provider)
            .with_limit(8),
    );

    assert_snapshot!("dml_cte_columns", render_completion(&result));
}

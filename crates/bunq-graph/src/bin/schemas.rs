use std::env;
use std::fs;
use std::path::PathBuf;

use schemars::schema_for;
use serde_json::{Value, json};

use bunq_graph::Bindings;

const GRAPH_SCHEMA_FILE: &str = "bunq-graph.schema.json";
const BINDINGS_SCHEMA_FILE: &str = "bunq-bindings.schema.json";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = env::args_os()
        .nth(1)
        .map_or(env::current_dir()?, PathBuf::from);
    fs::create_dir_all(&output_dir)?;

    let graph_schema = bunq_graph_schema()?;
    let bindings_schema = schema_for!(Bindings);

    write_schema(&output_dir.join(GRAPH_SCHEMA_FILE), &graph_schema)?;
    write_schema(&output_dir.join(BINDINGS_SCHEMA_FILE), &bindings_schema)?;

    println!(
        "wrote {} and {} to {}",
        GRAPH_SCHEMA_FILE,
        BINDINGS_SCHEMA_FILE,
        output_dir.display()
    );
    Ok(())
}

fn bunq_graph_schema() -> Result<Value, String> {
    let schema = schema_for!(clepsydra_lib::clepsydra_core::config::Config);
    let mut schema = serde_json::to_value(schema).map_err(|error| error.to_string())?;
    add_alias(&mut schema, "nodes", "accounts")?;
    add_alias(&mut schema, "flow", "transfers")?;
    add_alias(
        &mut schema,
        "default_bucket_target",
        "default_account_target",
    )?;
    add_alias(
        &mut schema,
        "default_flow_source",
        "default_transfer_source",
    )?;
    add_alias(
        &mut schema,
        "default_flow_target",
        "default_transfer_target",
    )?;

    allow_either_required(&mut schema, "nodes", "accounts")?;
    let flow_schema = schema
        .get_mut("definitions")
        .and_then(Value::as_object_mut)
        .and_then(|definitions| definitions.get_mut("FlowConfig"))
        .ok_or_else(|| "schema is missing FlowConfig".to_owned())?;
    add_alias(flow_schema, "volume", "amount")?;
    allow_either_required(flow_schema, "volume", "amount")?;

    Ok(schema)
}

fn add_alias(schema: &mut Value, canonical: &str, alias: &str) -> Result<(), String> {
    let properties = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "schema has no properties object".to_owned())?;
    let property = properties
        .get(canonical)
        .cloned()
        .ok_or_else(|| format!("schema has no {canonical} property"))?;
    properties.insert(alias.to_owned(), property);
    Ok(())
}

fn allow_either_required(schema: &mut Value, canonical: &str, alias: &str) -> Result<(), String> {
    let object = schema
        .as_object_mut()
        .ok_or_else(|| "schema is not an object".to_owned())?;
    let required = object
        .remove("required")
        .and_then(|value| value.as_array().cloned())
        .ok_or_else(|| "schema has no required array".to_owned())?;
    let Some(index) = required.iter().position(|value| value == canonical) else {
        object.insert("required".to_owned(), Value::Array(required));
        return Ok(());
    };
    let mut common_required = required;
    common_required.remove(index);
    let mut canonical_required = common_required.clone();
    canonical_required.push(Value::String(canonical.to_owned()));
    let mut alias_required = common_required;
    alias_required.push(Value::String(alias.to_owned()));
    object.insert(
        "anyOf".to_owned(),
        json!([
            { "required": canonical_required },
            { "required": alias_required }
        ]),
    );
    Ok(())
}

fn write_schema<T: serde::Serialize>(
    path: &std::path::Path,
    schema: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = serde_json::to_string_pretty(schema)?;
    fs::write(path, format!("{content}\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::bunq_graph_schema;

    #[test]
    fn graph_schema_contains_domain_aliases() {
        let schema = bunq_graph_schema().expect("graph schema generation failed");
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("graph schema has no properties object");
        assert!(properties.contains_key("accounts"));
        assert!(properties.contains_key("transfers"));
        assert!(properties.contains_key("default_account_target"));
        assert!(properties.contains_key("default_transfer_source"));
        assert!(properties.contains_key("default_transfer_target"));
        assert!(
            schema["definitions"]["FlowConfig"]["properties"]
                .get("amount")
                .is_some()
        );
    }

    #[test]
    fn graph_schema_allows_aliases_in_required_fields() {
        let Ok(schema) = bunq_graph_schema() else {
            panic!("graph schema generation failed");
        };
        let Some(required) = schema.get("anyOf").and_then(Value::as_array) else {
            panic!("root schema has no required alternatives");
        };
        assert_eq!(required.len(), 2);
        assert!(schema["definitions"]["FlowConfig"].get("anyOf").is_some());
    }
}

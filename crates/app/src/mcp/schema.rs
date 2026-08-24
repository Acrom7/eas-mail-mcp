use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use serde_json::{Map, Value};

/// Removes non-standard numeric formats rejected or logged by strict MCP clients.
pub(super) fn remove_numeric_formats<S>(router: &mut ToolRouter<S>) {
    for route in router.map.values_mut() {
        remove_from_object(Arc::make_mut(&mut route.attr.input_schema));
        if let Some(output_schema) = route.attr.output_schema.as_mut() {
            remove_from_object(Arc::make_mut(output_schema));
        }
    }
}

fn remove_from_object(object: &mut Map<String, Value>) {
    if object.get("type").is_some_and(is_numeric_type) {
        object.remove("format");
    }
    for value in object.values_mut() {
        remove_from_value(value);
    }
}

fn is_numeric_type(value: &Value) -> bool {
    match value {
        Value::Array(types) => types.iter().any(is_numeric_type),
        Value::String(value) => matches!(value.as_str(), "integer" | "number"),
        _ => false,
    }
}

fn remove_from_value(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                remove_from_value(item);
            }
        }
        Value::Object(object) => remove_from_object(object),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::remove_from_value;

    #[test]
    fn removes_nested_rust_numeric_formats_and_preserves_standard_formats() {
        let numeric_formats = [
            "double", "float", "int", "int8", "int16", "int32", "int64", "int128", "uint", "uint8",
            "uint16", "uint32", "uint64", "uint128",
        ];
        let properties: Map<String, Value> = numeric_formats
            .iter()
            .map(|format| ((*format).to_owned(), json!({ "type": "integer", "format": format })))
            .collect();
        let mut schema = json!({
            "type": "object",
            "properties": properties,
            "$defs": {
                "OptionalNumber": { "type": ["number", "null"], "format": "future-number" },
                "Timestamp": { "type": "string", "format": "date-time" }
            }
        });

        remove_from_value(&mut schema);

        for format in numeric_formats {
            assert_eq!(schema.pointer(&format!("/properties/{format}/format")), None);
        }
        assert_eq!(schema.pointer("/$defs/OptionalNumber/format"), None);
        assert_eq!(
            schema.pointer("/$defs/Timestamp/format").and_then(Value::as_str),
            Some("date-time")
        );
    }
}

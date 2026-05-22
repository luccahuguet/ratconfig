use serde_json::Value as JsonValue;

#[derive(Debug, Clone, PartialEq)]
pub enum MigrationOp {
    Rename { from: String, to: String },
    Delete { path: String },
    AddDefault { path: String, value: JsonValue },
}

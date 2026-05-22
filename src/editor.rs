use serde_json::Value as JsonValue;

#[derive(Debug, Clone, PartialEq)]
pub enum EditIntent {
    Set { path: String, value: JsonValue },
    Unset { path: String },
}

impl EditIntent {
    pub fn path(&self) -> &str {
        match self {
            Self::Set { path, .. } | Self::Unset { path } => path,
        }
    }
}

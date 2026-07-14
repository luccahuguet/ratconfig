// Test lane: default

use serde_json::Value as JsonValue;

pub type ValueTransform = fn(&JsonValue) -> Result<Option<JsonValue>, String>;

#[derive(Debug, Clone)]
pub enum MigrationOp {
    Rename {
        from: String,
        to: String,
    },
    Delete {
        path: String,
    },
    AddDefault {
        path: String,
        value: JsonValue,
    },
    Transform {
        path: String,
        transform: ValueTransform,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    pub text: String,
    pub mutations: Vec<MigrationMutation>,
}

impl MigrationOutcome {
    pub fn changed(&self) -> bool {
        !self.mutations.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationMutation {
    Renamed { from: String, to: String },
    Deleted { path: String },
    AddedDefault { path: String },
    Transformed { path: String },
}

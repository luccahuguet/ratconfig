#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigModel {
    pub tabs: Vec<String>,
    pub fields: Vec<ConfigField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigField {
    pub path: String,
    pub tab: String,
    pub kind: String,
    pub value: String,
    pub state: ValueState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueState {
    Explicit,
    Defaulted,
    Unset,
    Invalid,
}

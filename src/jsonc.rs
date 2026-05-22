#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchOutcome {
    pub text: String,
    pub changed: bool,
}

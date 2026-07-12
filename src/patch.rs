use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchMutation {
    Inserted,
    Replaced,
    Removed,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchOutcome {
    pub text: String,
    pub mutation: PatchMutation,
}

impl PatchOutcome {
    pub fn changed(&self) -> bool {
        self.mutation != PatchMutation::Unchanged
    }
}

pub fn split_dotted_path(path: &str) -> Option<Vec<String>> {
    path.split('.')
        .map(|part| {
            let part = part.trim();
            (!part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'))
            .then(|| part.to_owned())
        })
        .collect()
}

pub fn dotted_paths_overlap(a: &str, b: &str) -> bool {
    let (Some(a), Some(b)) = (split_dotted_path(a), split_dotted_path(b)) else {
        return false;
    };
    (a.len() < b.len() && b.starts_with(&a)) || (b.len() < a.len() && a.starts_with(&b))
}

pub(crate) fn get_dotted_json_path<'a>(value: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    path.split('.')
        .try_fold(value, |current, part| current.as_object()?.get(part))
}

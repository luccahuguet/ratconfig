use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchMutation {
    Inserted,
    Replaced,
    Removed,
    Unchanged,
}

pub fn split_dotted_path(path: &str) -> Option<Vec<String>> {
    let parts = path
        .split('.')
        .map(|part| part.trim().to_owned())
        .collect::<Vec<_>>();
    if parts.iter().any(|part| {
        part.is_empty()
            || part.contains('[')
            || part.contains(']')
            || !part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    }) {
        return None;
    }
    Some(parts)
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

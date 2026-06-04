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
        .map(str::trim)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if parts.is_empty()
        || parts.iter().any(|part| {
            part.is_empty()
                || part.contains('[')
                || part.contains(']')
                || !part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
    {
        return None;
    }
    Some(parts)
}

pub fn dotted_paths_overlap(a: &str, b: &str) -> bool {
    let Some(a) = split_dotted_path(a) else {
        return false;
    };
    let Some(b) = split_dotted_path(b) else {
        return false;
    };
    is_strict_prefix(&a, &b) || is_strict_prefix(&b, &a)
}

fn is_strict_prefix(prefix: &[String], path: &[String]) -> bool {
    prefix.len() < path.len() && path.starts_with(prefix)
}

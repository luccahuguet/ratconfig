use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    InvalidJsonc { source: String },
    InvalidPath { path: String },
    RewriteRequired { path: String, detail: String },
    UnsupportedValue { path: String, detail: String },
}

pub fn set_jsonc_value_text(
    raw: &str,
    path: &str,
    value: &JsonValue,
) -> Result<PatchOutcome, PatchError> {
    let parts = split_path(path)?;
    let replacement = cst_input_from_json_value(value, path)?;
    let root = parse_cst(raw)?;
    let root_object = root.object_value_or_create().ok_or_else(|| {
        rewrite_required(
            path,
            "The document root is not a JSON object, so this patch cannot be applied without rewriting the file.",
        )
    })?;
    let parent = parent_object_or_create(root_object, &parts, path)?;
    let leaf = parts.last().expect("split path guarantees a leaf");
    let mutation = match parent.get(leaf) {
        Some(prop) => {
            prop.set_value(replacement);
            PatchMutation::Replaced
        }
        None => {
            parent.append(leaf, replacement);
            PatchMutation::Inserted
        }
    };
    let text = root.to_string();
    let mutation = if text == raw {
        PatchMutation::Unchanged
    } else {
        mutation
    };
    validate_jsonc(&text)?;
    Ok(PatchOutcome { text, mutation })
}

pub fn unset_jsonc_value_text(raw: &str, path: &str) -> Result<PatchOutcome, PatchError> {
    let parts = split_path(path)?;
    let root = parse_cst(raw)?;
    let Some(root_object) = root.object_value() else {
        return Ok(PatchOutcome {
            text: raw.to_string(),
            mutation: PatchMutation::Unchanged,
        });
    };
    let Some(parent) = parent_object_if_present(root_object, &parts, path)? else {
        return Ok(PatchOutcome {
            text: raw.to_string(),
            mutation: PatchMutation::Unchanged,
        });
    };
    let leaf = parts.last().expect("split path guarantees a leaf");
    let Some(prop) = parent.get(leaf) else {
        return Ok(PatchOutcome {
            text: raw.to_string(),
            mutation: PatchMutation::Unchanged,
        });
    };
    prop.remove();
    let text = root.to_string();
    validate_jsonc(&text)?;
    Ok(PatchOutcome {
        text,
        mutation: PatchMutation::Removed,
    })
}

pub fn parse_jsonc_value(raw: &str) -> Result<JsonValue, PatchError> {
    jsonc_parser::parse_to_serde_value::<JsonValue>(raw, &jsonc_parse_options()).map_err(|source| {
        PatchError::InvalidJsonc {
            source: source.to_string(),
        }
    })
}

pub fn get_json_path<'a>(value: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut current = value;
    for part in path.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

pub fn jsonc_parse_options() -> ParseOptions {
    ParseOptions {
        allow_comments: true,
        allow_loose_object_property_names: false,
        allow_trailing_commas: true,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
    }
}

fn parse_cst(raw: &str) -> Result<CstRootNode, PatchError> {
    CstRootNode::parse(raw, &jsonc_parse_options()).map_err(|source| PatchError::InvalidJsonc {
        source: source.to_string(),
    })
}

fn validate_jsonc(raw: &str) -> Result<(), PatchError> {
    parse_jsonc_value(raw).map(|_| ())
}

fn split_path(path: &str) -> Result<Vec<String>, PatchError> {
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
        return Err(PatchError::InvalidPath {
            path: path.to_string(),
        });
    }
    Ok(parts)
}

fn parent_object_or_create(
    root_object: CstObject,
    parts: &[String],
    path: &str,
) -> Result<CstObject, PatchError> {
    let mut current = root_object;
    for part in &parts[..parts.len().saturating_sub(1)] {
        current = current.object_value_or_create(part).ok_or_else(|| {
            rewrite_required(
                path,
                "A parent path exists but is not a JSON object, so ratconfig cannot patch through it safely.",
            )
        })?;
    }
    Ok(current)
}

fn parent_object_if_present(
    root_object: CstObject,
    parts: &[String],
    path: &str,
) -> Result<Option<CstObject>, PatchError> {
    let mut current = root_object;
    for part in &parts[..parts.len().saturating_sub(1)] {
        let Some(prop) = current.get(part) else {
            return Ok(None);
        };
        let Some(value) = prop.value() else {
            return Err(rewrite_required(
                path,
                "A parent path has no value, so ratconfig cannot remove through it safely.",
            ));
        };
        let Some(object) = value.as_object() else {
            return Err(rewrite_required(
                path,
                "A parent path exists but is not a JSON object, so ratconfig cannot remove through it safely.",
            ));
        };
        current = object;
    }
    Ok(Some(current))
}

fn cst_input_from_json_value(value: &JsonValue, path: &str) -> Result<CstInputValue, PatchError> {
    match value {
        JsonValue::Null => Ok(CstInputValue::Null),
        JsonValue::Bool(value) => Ok(CstInputValue::Bool(*value)),
        JsonValue::Number(value) => Ok(CstInputValue::Number(value.to_string())),
        JsonValue::String(value) => Ok(CstInputValue::String(value.clone())),
        JsonValue::Array(values) => {
            let mut items = Vec::new();
            for value in values {
                let Some(value) = value.as_str() else {
                    return Err(unsupported_value(
                        path,
                        "Only arrays of strings are supported by the safe JSONC patcher.",
                    ));
                };
                items.push(CstInputValue::String(value.to_string()));
            }
            Ok(CstInputValue::Array(items))
        }
        JsonValue::Object(object) => {
            let mut properties = Vec::new();
            for (key, value) in object {
                properties.push((key.clone(), cst_input_from_json_value(value, path)?));
            }
            Ok(CstInputValue::Object(properties))
        }
    }
}

fn unsupported_value(path: &str, detail: &str) -> PatchError {
    PatchError::UnsupportedValue {
        path: path.to_string(),
        detail: detail.to_string(),
    }
}

fn rewrite_required(path: &str, detail: &str) -> PatchError {
    PatchError::RewriteRequired {
        path: path.to_string(),
        detail: detail.to_string(),
    }
}

//! Coded validation issues (SPEC.md §7).
//!
//! Every validation failure carries a stable code, a JSON Pointer path into
//! the document, and a message that names the offending value. Library code
//! never panics; all fallible entry points return `Result<_, Issue>`.

use crate::math::Vec3;
use serde_json::Value;

/// A single validation issue (code + path + message).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {path}: {message}")]
pub struct Issue {
    /// Stable public code, e.g. `T-03`, `W-06`, `C-01`, `P-01` (SPEC.md §7).
    pub code: &'static str,
    /// JSON Pointer path into the offending document, or a file path.
    pub path: String,
    /// Human-readable message naming the offending value.
    pub message: String,
}

impl Issue {
    pub fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

/// Parse a trajectory document that may be JSON or YAML (SPEC.md §2) into a
/// generic JSON value. JSON is tried first; YAML (converted through
/// `serde_yaml::Value`) is the fallback. Malformed input is `P-01`.
pub fn parse_document(text: &str) -> Result<serde_json::Value, Issue> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        return Ok(v);
    }
    let yaml: serde_yaml::Value = serde_yaml::from_str(text).map_err(|yaml_err| {
        Issue::new(
            "P-01",
            "/",
            format!("document is neither valid JSON nor valid YAML: {yaml_err}"),
        )
    })?;
    serde_json::to_value(yaml).map_err(|err| {
        Issue::new(
            "P-01",
            "/",
            format!("YAML document cannot be represented as JSON (keys must be strings): {err}"),
        )
    })
}

/// Extract a finite `f32` from a JSON value. Missing/wrong type/non-finite
/// values produce `code` with `path` and the offending value in the message.
pub fn value_f32(v: &Value, path: &str, code: &'static str) -> Result<f32, Issue> {
    let n = v
        .as_f64()
        .ok_or_else(|| Issue::new(code, path, format!("expected a number, got {v}")))?;
    let f = n as f32;
    if !f.is_finite() {
        return Err(Issue::new(
            code,
            path,
            format!("expected a finite number, got {n}"),
        ));
    }
    Ok(f)
}

/// Extract exactly `n` finite `f32`s from a JSON array. Wrong length, wrong
/// types, and non-finite members produce `code` with `path`.
pub fn value_vecn(v: &Value, path: &str, code: &'static str, n: usize) -> Result<Vec<f32>, Issue> {
    let arr = v.as_array().ok_or_else(|| {
        Issue::new(
            code,
            path,
            format!("expected an array of {n} numbers, got {v}"),
        )
    })?;
    if arr.len() != n {
        return Err(Issue::new(
            code,
            path,
            format!("expected an array of {n} numbers, got length {}", arr.len()),
        ));
    }
    arr.iter()
        .enumerate()
        .map(|(i, e)| value_f32(e, &format!("{path}/{i}"), code))
        .collect()
}

/// Extract a `Vec3` (three finite `f32`s) from a JSON array.
pub fn value_vec3(v: &Value, path: &str, code: &'static str) -> Result<Vec3, Issue> {
    let a = value_vecn(v, path, code, 3)?;
    Ok(Vec3::new(a[0], a[1], a[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_json() {
        let v = parse_document(r#"{"version": 1, "head": []}"#).unwrap();
        assert_eq!(v["version"], serde_json::json!(1));
    }

    #[test]
    fn parses_yaml_fallback() {
        let v = parse_document("version: 1\nhead: []\n").unwrap();
        assert_eq!(v["version"], serde_json::json!(1));
        assert!(v["head"].is_array());
    }

    #[test]
    fn rejects_garbage_with_p01() {
        let err = parse_document("not: [a, document").unwrap_err();
        assert_eq!(err.code, "P-01");
        assert_eq!(err.path, "/");
        assert!(err.message.contains("neither valid JSON nor valid YAML"));
    }

    #[test]
    fn issue_displays_code_path_message() {
        let issue = Issue::new(
            "T-02",
            "/head/1/t_sec",
            "t_sec must be strictly increasing: got 1.0 after 2.0",
        );
        assert_eq!(
            issue.to_string(),
            "T-02: /head/1/t_sec: t_sec must be strictly increasing: got 1.0 after 2.0"
        );
    }
}

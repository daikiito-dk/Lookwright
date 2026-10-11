//! Assertion specs and their config parsing (SPEC.md §9; codes A-01…A-05,
//! validation codes C-05/C-06).

use std::path::{Path, PathBuf};

use lookwright_core::error::{value_f32, Issue};
use lookwright_core::trajectory::EVENT_TYPES;
use serde_json::Value;

/// One assertion from a test config (SPEC.md §9).
#[derive(Debug, Clone, PartialEq)]
pub enum AssertionSpec {
    /// A-01 `ui_visible`
    UiVisible {
        ui: String,
        duration_sec: f32,
        min_coverage: f32,
        require_unoccluded: bool,
    },
    /// A-02 `gaze_focused`
    GazeFocused { target: String, duration_sec: f32 },
    /// A-03 `anchor_drift`
    AnchorDrift { ui: String, max_drift_m: f32 },
    /// A-04 `event_fired`
    EventFired { event: String, name: Option<String> },
    /// A-05 `golden_visual`
    GoldenVisual {
        dir: PathBuf,
        times_sec: Vec<f32>,
        max_mean_abs_diff: f32,
        max_changed_pixel_ratio: f32,
    },
}

impl AssertionSpec {
    /// Stable assertion code (SPEC.md §7/§9).
    pub fn code(&self) -> &'static str {
        match self {
            AssertionSpec::UiVisible { .. } => "A-01",
            AssertionSpec::GazeFocused { .. } => "A-02",
            AssertionSpec::AnchorDrift { .. } => "A-03",
            AssertionSpec::EventFired { .. } => "A-04",
            AssertionSpec::GoldenVisual { .. } => "A-05",
        }
    }

    /// Config `type` string.
    pub fn kind(&self) -> &'static str {
        match self {
            AssertionSpec::UiVisible { .. } => "ui_visible",
            AssertionSpec::GazeFocused { .. } => "gaze_focused",
            AssertionSpec::AnchorDrift { .. } => "anchor_drift",
            AssertionSpec::EventFired { .. } => "event_fired",
            AssertionSpec::GoldenVisual { .. } => "golden_visual",
        }
    }

    /// Referenced UI element id, if any (for report/junit names).
    pub fn ui_ref(&self) -> Option<&str> {
        match self {
            AssertionSpec::UiVisible { ui, .. } | AssertionSpec::AnchorDrift { ui, .. } => Some(ui),
            _ => None,
        }
    }

    /// The field name the UI reference lives in (for error paths).
    pub fn ui_ref_field(&self) -> Option<&'static str> {
        match self {
            AssertionSpec::UiVisible { .. } | AssertionSpec::AnchorDrift { .. } => Some("ui"),
            _ => None,
        }
    }

    /// Referenced world object id, if any (gaze targets).
    pub fn gaze_ref(&self) -> Option<&str> {
        match self {
            AssertionSpec::GazeFocused { target, .. } => Some(target),
            _ => None,
        }
    }

    /// Parse one assertion from a config fragment (SPEC.md §6/§9).
    /// `base_dir` resolves relative golden `dir` paths.
    pub fn parse(v: &Value, path: &str, base_dir: &Path) -> Result<Self, Issue> {
        let kind = v.get("type").and_then(Value::as_str).ok_or_else(|| {
            Issue::new(
                "C-05",
                format!("{path}/type"),
                format!("assertion requires a string type, got {v}"),
            )
        })?;
        match kind {
            "ui_visible" => {
                let ui = required_str(v, "ui", path, "C-05")?;
                let duration_sec = opt_f32(v, "duration_sec", path, "C-05", 1.0)?;
                if duration_sec <= 0.0 {
                    return Err(Issue::new(
                        "C-05",
                        format!("{path}/duration_sec"),
                        format!("duration_sec must be > 0, got {duration_sec}"),
                    ));
                }
                let min_coverage = opt_f32(v, "min_coverage", path, "C-05", 0.5)?;
                if !(0.0..=1.0).contains(&min_coverage) {
                    return Err(Issue::new(
                        "C-05",
                        format!("{path}/min_coverage"),
                        format!("min_coverage must be within [0, 1], got {min_coverage}"),
                    ));
                }
                let require_unoccluded = opt_bool(v, "require_unoccluded", path, "C-05")?;
                Ok(AssertionSpec::UiVisible {
                    ui,
                    duration_sec,
                    min_coverage,
                    require_unoccluded,
                })
            }
            "gaze_focused" => {
                let target = required_str(v, "target", path, "C-05")?;
                let duration_sec = opt_f32(v, "duration_sec", path, "C-05", 1.0)?;
                if duration_sec <= 0.0 {
                    return Err(Issue::new(
                        "C-05",
                        format!("{path}/duration_sec"),
                        format!("duration_sec must be > 0, got {duration_sec}"),
                    ));
                }
                Ok(AssertionSpec::GazeFocused {
                    target,
                    duration_sec,
                })
            }
            "anchor_drift" => {
                let ui = required_str(v, "ui", path, "C-05")?;
                let max_drift_m = opt_f32(v, "max_drift_m", path, "C-05", 0.01)?;
                if max_drift_m < 0.0 {
                    return Err(Issue::new(
                        "C-05",
                        format!("{path}/max_drift_m"),
                        format!("max_drift_m must be >= 0, got {max_drift_m}"),
                    ));
                }
                Ok(AssertionSpec::AnchorDrift { ui, max_drift_m })
            }
            "event_fired" => {
                let event = required_str(v, "event", path, "C-05")?;
                if !EVENT_TYPES.contains(&event.as_str()) {
                    return Err(Issue::new(
                        "C-05",
                        format!("{path}/event"),
                        format!(
                            "unknown event type {event:?}; expected one of {EVENT_TYPES:?}"
                        ),
                    ));
                }
                let name = match v.get("name") {
                    None => None,
                    Some(nv) if nv.is_null() => None,
                    Some(nv) => Some(
                        nv.as_str()
                            .ok_or_else(|| {
                                Issue::new(
                                    "C-05",
                                    format!("{path}/name"),
                                    format!("name must be a string, got {nv}"),
                                )
                            })?
                            .to_string(),
                    ),
                };
                Ok(AssertionSpec::EventFired { event, name })
            }
            "golden_visual" => {
                let dir_s = required_str(v, "dir", path, "C-06")?;
                let dir = base_dir.join(dir_s);
                let times_sec = match v.get("times_sec").and_then(Value::as_array) {
                    Some(arr) if !arr.is_empty() => {
                        let mut out = Vec::with_capacity(arr.len());
                        for (i, tv) in arr.iter().enumerate() {
                            let t = value_f32(tv, &format!("{path}/times_sec/{i}"), "C-06")?;
                            if t < 0.0 {
                                return Err(Issue::new(
                                    "C-06",
                                    format!("{path}/times_sec/{i}"),
                                    format!("times_sec entries must be >= 0, got {t}"),
                                ));
                            }
                            out.push(t);
                        }
                        out
                    }
                    _ => {
                        return Err(Issue::new(
                            "C-06",
                            format!("{path}/times_sec"),
                            "golden_visual requires a non-empty times_sec array",
                        ));
                    }
                };
                let max_mean_abs_diff = opt_f32(v, "max_mean_abs_diff", path, "C-06", 1.5)?;
                if max_mean_abs_diff < 0.0 {
                    return Err(Issue::new(
                        "C-06",
                        format!("{path}/max_mean_abs_diff"),
                        format!("max_mean_abs_diff must be >= 0, got {max_mean_abs_diff}"),
                    ));
                }
                let max_changed_pixel_ratio =
                    opt_f32(v, "max_changed_pixel_ratio", path, "C-06", 0.02)?;
                if !(0.0..=1.0).contains(&max_changed_pixel_ratio) {
                    return Err(Issue::new(
                        "C-06",
                        format!("{path}/max_changed_pixel_ratio"),
                        format!(
                            "max_changed_pixel_ratio must be within [0, 1], got {max_changed_pixel_ratio}"
                        ),
                    ));
                }
                Ok(AssertionSpec::GoldenVisual {
                    dir,
                    times_sec,
                    max_mean_abs_diff,
                    max_changed_pixel_ratio,
                })
            }
            other => Err(Issue::new(
                "C-05",
                format!("{path}/type"),
                format!(
                    "unknown assertion type {other:?}; expected ui_visible, gaze_focused, anchor_drift, event_fired, or golden_visual"
                ),
            )),
        }
    }
}

fn required_str(v: &Value, field: &str, path: &str, code: &'static str) -> Result<String, Issue> {
    v.get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            let got = v.get(field).cloned().unwrap_or(Value::Null);
            Issue::new(
                code,
                format!("{path}/{field}"),
                format!("missing required string field {field:?}, got {got}"),
            )
        })
}

fn opt_f32(
    v: &Value,
    field: &str,
    path: &str,
    code: &'static str,
    default: f32,
) -> Result<f32, Issue> {
    match v.get(field) {
        None => Ok(default),
        Some(x) if x.is_null() => Ok(default),
        Some(x) => value_f32(x, &format!("{path}/{field}"), code),
    }
}

fn opt_bool(v: &Value, field: &str, path: &str, code: &'static str) -> Result<bool, Issue> {
    match v.get(field) {
        None => Ok(false),
        Some(x) if x.is_null() => Ok(false),
        Some(x) => x.as_bool().ok_or_else(|| {
            Issue::new(
                code,
                format!("{path}/{field}"),
                format!("{field} must be a boolean, got {x}"),
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> Result<AssertionSpec, Issue> {
        AssertionSpec::parse(&v, "/assertions/0", Path::new("/tmp"))
    }

    #[test]
    fn parses_all_five_kinds_with_defaults() {
        let a = parse(json!({ "type": "ui_visible", "ui": "card" })).unwrap();
        assert_eq!(a.code(), "A-01");
        match &a {
            AssertionSpec::UiVisible {
                ui,
                duration_sec,
                min_coverage,
                require_unoccluded,
            } => {
                assert_eq!(ui, "card");
                assert_eq!(*duration_sec, 1.0);
                assert_eq!(*min_coverage, 0.5);
                assert!(!require_unoccluded);
            }
            _ => panic!("wrong kind"),
        }

        let a = parse(json!({ "type": "gaze_focused", "target": "tv" })).unwrap();
        assert_eq!(a.code(), "A-02");
        assert_eq!(a.gaze_ref(), Some("tv"));

        let a = parse(json!({ "type": "anchor_drift", "ui": "card" })).unwrap();
        assert_eq!(a.code(), "A-03");
        match &a {
            AssertionSpec::AnchorDrift { max_drift_m, .. } => assert_eq!(*max_drift_m, 0.01),
            _ => panic!("wrong kind"),
        }

        let a = parse(json!({ "type": "event_fired", "event": "gesture" })).unwrap();
        assert_eq!(a.code(), "A-04");

        let a = parse(json!({
            "type": "golden_visual", "dir": "gold", "times_sec": [1.0, 4.0]
        }))
        .unwrap();
        assert_eq!(a.code(), "A-05");
        match &a {
            AssertionSpec::GoldenVisual {
                dir,
                times_sec,
                max_mean_abs_diff,
                max_changed_pixel_ratio,
            } => {
                assert_eq!(dir, Path::new("/tmp/gold"));
                assert_eq!(times_sec, &vec![1.0, 4.0]);
                assert_eq!(*max_mean_abs_diff, 1.5);
                assert_eq!(*max_changed_pixel_ratio, 0.02);
            }
            _ => panic!("wrong kind"),
        }
    }

    #[test]
    fn c05_unknown_type_and_bad_params() {
        let e = parse(json!({ "type": "ui_invisible" })).unwrap_err();
        assert_eq!(e.code, "C-05");
        assert_eq!(e.path, "/assertions/0/type");

        let e = parse(json!({ "type": "ui_visible" })).unwrap_err();
        assert_eq!(e.code, "C-05");
        assert_eq!(e.path, "/assertions/0/ui");

        let e =
            parse(json!({ "type": "ui_visible", "ui": "card", "duration_sec": 0.0 })).unwrap_err();
        assert_eq!(e.code, "C-05");
        assert_eq!(e.path, "/assertions/0/duration_sec");

        let e = parse(json!({ "type": "event_fired", "event": "tap" })).unwrap_err();
        assert_eq!(e.code, "C-05");
        assert_eq!(e.path, "/assertions/0/event");
    }

    #[test]
    fn c06_golden_params() {
        let e = parse(json!({ "type": "golden_visual", "dir": "g", "times_sec": [] })).unwrap_err();
        assert_eq!(e.code, "C-06");
        assert_eq!(e.path, "/assertions/0/times_sec");

        let e = parse(json!({
            "type": "golden_visual", "dir": "g", "times_sec": [-1.0]
        }))
        .unwrap_err();
        assert_eq!(e.code, "C-06");
        assert_eq!(e.path, "/assertions/0/times_sec/0");

        let e = parse(json!({
            "type": "golden_visual", "dir": "g", "times_sec": [1.0],
            "max_changed_pixel_ratio": 1.5
        }))
        .unwrap_err();
        assert_eq!(e.code, "C-06");
        assert_eq!(e.path, "/assertions/0/max_changed_pixel_ratio");
    }
}

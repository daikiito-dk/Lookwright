//! 6DoF trajectory parsing, validation, and sampling (SPEC.md §2, §13).
//!
//! JSON and YAML input, coded validation (`T-01`…`T-06`), linear position /
//! SLERP rotation interpolation with clamped holds outside the keyframe range.

use crate::camera::CameraPose;
use crate::error::{value_f32, value_vecn, Issue};
use crate::math::{Quat, Vec3};
use serde_json::Value;

/// Event types accepted in v0.1 (SPEC.md §2).
pub const EVENT_TYPES: [&str; 3] = ["gaze_at", "gesture", "voice"];

/// An injected input event at a specific time (SPEC.md §2 `events`).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub t_sec: f32,
    /// One of [`EVENT_TYPES`].
    pub kind: String,
    /// `gaze_at` target object id.
    pub target: Option<String>,
    /// `gesture` name, e.g. `pinch`.
    pub name: Option<String>,
    /// `voice` transcript.
    pub transcript: Option<String>,
}

/// The composed head + gaze state at one point in time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub t_sec: f32,
    /// Head pose composed with head-local gaze rotation (SPEC.md §1).
    pub camera: CameraPose,
    pub gaze_yaw_deg: f32,
    pub gaze_pitch_deg: f32,
}

#[derive(Debug, Clone, Copy)]
struct HeadKey {
    t_sec: f32,
    position_m: Vec3,
    rotation: Quat,
}

#[derive(Debug, Clone, Copy)]
struct GazeKey {
    t_sec: f32,
    yaw_deg: f32,
    pitch_deg: f32,
}

/// A validated 6DoF trajectory.
#[derive(Debug, Clone)]
pub struct Trajectory {
    head: Vec<HeadKey>,
    gaze: Vec<GazeKey>,
    events: Vec<Event>,
    duration_sec: f32,
}

impl Trajectory {
    /// Parse and validate a trajectory from JSON or YAML text (SPEC.md §2).
    pub fn parse(text: &str) -> Result<Self, Issue> {
        let doc = crate::error::parse_document(text)?;
        Self::from_value(&doc)
    }

    pub fn from_value(doc: &Value) -> Result<Self, Issue> {
        let head = Self::parse_head(doc)?;
        let gaze = Self::parse_gaze(doc)?;
        let events = Self::parse_events(doc)?;

        let max_t = head
            .last()
            .map(|k| k.t_sec)
            .unwrap_or(0.0)
            .max(gaze.last().map(|k| k.t_sec).unwrap_or(0.0))
            .max(events.last().map(|e| e.t_sec).unwrap_or(0.0));
        let duration_sec = match doc.get("duration_sec") {
            Some(dv) => {
                let d = value_f32(dv, "/duration_sec", "T-04")?;
                if d < max_t {
                    return Err(Issue::new(
                        "T-06",
                        "/duration_sec",
                        format!("duration_sec {d} is smaller than the largest time {max_t}"),
                    ));
                }
                d
            }
            None => max_t,
        };

        Ok(Self {
            head,
            gaze,
            events,
            duration_sec,
        })
    }

    fn parse_head(doc: &Value) -> Result<Vec<HeadKey>, Issue> {
        let empty = || Issue::new("T-01", "/head", "head section is missing or empty");
        let arr = doc
            .get("head")
            .ok_or_else(empty)?
            .as_array()
            .ok_or_else(empty)?;
        if arr.is_empty() {
            return Err(empty());
        }
        let mut keys = Vec::with_capacity(arr.len());
        let mut prev_t: Option<f32> = None;
        for (i, sv) in arr.iter().enumerate() {
            let base = format!("/head/{i}");
            let t = value_f32(
                sv.get("t_sec").ok_or_else(|| {
                    Issue::new(
                        "T-04",
                        format!("{base}/t_sec"),
                        format!("missing t_sec in {sv}"),
                    )
                })?,
                &format!("{base}/t_sec"),
                "T-04",
            )?;
            if let Some(p) = prev_t {
                if t <= p {
                    return Err(Issue::new(
                        "T-02",
                        format!("{base}/t_sec"),
                        format!("t_sec must be strictly increasing: got {t} after {p}"),
                    ));
                }
            }
            prev_t = Some(t);
            let pos_arr = sv.get("position_m").ok_or_else(|| {
                Issue::new(
                    "T-04",
                    format!("{base}/position_m"),
                    format!("missing position_m in {sv}"),
                )
            })?;
            let pos_v = value_vecn(pos_arr, &format!("{base}/position_m"), "T-04", 3)?;
            let position_m = Vec3::new(pos_v[0], pos_v[1], pos_v[2]);
            let rotation = Self::parse_rotation(sv, &base)?;
            keys.push(HeadKey {
                t_sec: t,
                position_m,
                rotation,
            });
        }
        Ok(keys)
    }

    fn parse_rotation(sv: &Value, base: &str) -> Result<Quat, Issue> {
        let has_q = sv.get("rotation_quat").is_some();
        let has_e = sv.get("rotation_euler_ypr_deg").is_some();
        match (has_q, has_e) {
            (true, true) => Err(Issue::new(
                "T-03",
                base,
                "sample has both rotation_quat and rotation_euler_ypr_deg; provide exactly one",
            )),
            (false, false) => Ok(Quat::IDENTITY),
            (true, false) => {
                let path = format!("{base}/rotation_quat");
                let q = sv.get("rotation_quat").unwrap();
                let a = value_vecn(q, &path, "T-03", 4)?;
                let len = Quat::dot(
                    Quat::new(a[0], a[1], a[2], a[3]),
                    Quat::new(a[0], a[1], a[2], a[3]),
                )
                .sqrt();
                if len == 0.0 {
                    return Err(Issue::new(
                        "T-03",
                        path,
                        format!("zero-length quaternion {a:?}"),
                    ));
                }
                Ok(Quat::new(a[0], a[1], a[2], a[3]).normalized())
            }
            (false, true) => {
                let path = format!("{base}/rotation_euler_ypr_deg");
                let e = sv.get("rotation_euler_ypr_deg").unwrap();
                let a = value_vecn(e, &path, "T-03", 3)?;
                Ok(Quat::from_euler_ypr_deg(a[0], a[1], a[2]))
            }
        }
    }

    fn parse_gaze(doc: &Value) -> Result<Vec<GazeKey>, Issue> {
        let mut keys = Vec::new();
        let Some(gv) = doc.get("gaze") else {
            return Ok(keys);
        };
        let arr = gv.as_array().ok_or_else(|| {
            Issue::new(
                "T-04",
                "/gaze",
                format!("gaze section must be an array, got {gv}"),
            )
        })?;
        let mut prev_t: Option<f32> = None;
        for (i, gv) in arr.iter().enumerate() {
            let base = format!("/gaze/{i}");
            let t = value_f32(
                gv.get("t_sec").ok_or_else(|| {
                    Issue::new(
                        "T-04",
                        format!("{base}/t_sec"),
                        format!("missing t_sec in {gv}"),
                    )
                })?,
                &format!("{base}/t_sec"),
                "T-04",
            )?;
            if let Some(p) = prev_t {
                if t <= p {
                    return Err(Issue::new(
                        "T-02",
                        format!("{base}/t_sec"),
                        format!("t_sec must be strictly increasing: got {t} after {p}"),
                    ));
                }
            }
            prev_t = Some(t);
            let yaw = match gv.get("yaw_deg") {
                Some(v) => value_f32(v, &format!("{base}/yaw_deg"), "T-04")?,
                None => 0.0,
            };
            let pitch = match gv.get("pitch_deg") {
                Some(v) => value_f32(v, &format!("{base}/pitch_deg"), "T-04")?,
                None => 0.0,
            };
            keys.push(GazeKey {
                t_sec: t,
                yaw_deg: yaw,
                pitch_deg: pitch,
            });
        }
        Ok(keys)
    }

    fn parse_events(doc: &Value) -> Result<Vec<Event>, Issue> {
        let mut events = Vec::new();
        let Some(evs) = doc.get("events") else {
            return Ok(events);
        };
        let arr = evs.as_array().ok_or_else(|| {
            Issue::new(
                "T-05",
                "/events",
                format!("events section must be an array, got {evs}"),
            )
        })?;
        let mut prev_t: Option<f32> = None;
        for (i, ev) in arr.iter().enumerate() {
            let base = format!("/events/{i}");
            let t = value_f32(
                ev.get("t_sec").ok_or_else(|| {
                    Issue::new(
                        "T-04",
                        format!("{base}/t_sec"),
                        format!("missing t_sec in {ev}"),
                    )
                })?,
                &format!("{base}/t_sec"),
                "T-04",
            )?;
            if let Some(p) = prev_t {
                if t <= p {
                    return Err(Issue::new(
                        "T-02",
                        format!("{base}/t_sec"),
                        format!("t_sec must be strictly increasing: got {t} after {p}"),
                    ));
                }
            }
            prev_t = Some(t);

            let kind_val = ev.get("type");
            let kind = kind_val.and_then(Value::as_str).ok_or_else(|| {
                Issue::new(
                    "T-05",
                    format!("{base}/type"),
                    format!("event requires a string type, got {kind_val:?}"),
                )
            })?;
            let (target, name, transcript) = match kind {
                "gaze_at" => {
                    let target = ev.get("target").and_then(Value::as_str).ok_or_else(|| {
                        Issue::new(
                            "T-05",
                            format!("{base}/target"),
                            format!("gaze_at event requires a target, got {ev}"),
                        )
                    })?;
                    (Some(target.to_string()), None, None)
                }
                "gesture" => {
                    let name = ev.get("name").and_then(Value::as_str).ok_or_else(|| {
                        Issue::new(
                            "T-05",
                            format!("{base}/name"),
                            format!("gesture event requires a name, got {ev}"),
                        )
                    })?;
                    (None, Some(name.to_string()), None)
                }
                "voice" => {
                    let transcript =
                        ev.get("transcript")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                Issue::new(
                                    "T-05",
                                    format!("{base}/transcript"),
                                    format!("voice event requires a transcript, got {ev}"),
                                )
                            })?;
                    (None, None, Some(transcript.to_string()))
                }
                other => {
                    return Err(Issue::new(
                        "T-05",
                        format!("{base}/type"),
                        format!("unknown event type {other:?}; expected one of {EVENT_TYPES:?}"),
                    ));
                }
            };
            events.push(Event {
                t_sec: t,
                kind: kind.to_string(),
                target,
                name,
                transcript,
            });
        }
        Ok(events)
    }

    /// Run duration (SPEC.md §2): explicit `duration_sec` or `max(t_sec)`.
    pub fn duration_sec(&self) -> f32 {
        self.duration_sec
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Events in the interval `(lo_sec, hi_sec]`.
    pub fn events_between(&self, lo_sec: f32, hi_sec: f32) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| e.t_sec > lo_sec && e.t_sec <= hi_sec)
            .collect()
    }

    /// Composed head+gaze state at `t_sec` (SPEC.md §2 interpolation rules).
    pub fn sample(&self, t_sec: f32) -> Sample {
        let (pos, head_rot) = interp_head(&self.head, t_sec);
        let (yaw, pitch) = interp_gaze(&self.gaze, t_sec);
        let camera_rot = head_rot
            * Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), yaw)
            * Quat::from_axis_angle_deg(Vec3::new(1.0, 0.0, 0.0), pitch);
        Sample {
            t_sec,
            camera: CameraPose::new(pos, camera_rot),
            gaze_yaw_deg: yaw,
            gaze_pitch_deg: pitch,
        }
    }
}

fn interp_head(keys: &[HeadKey], t: f32) -> (Vec3, Quat) {
    let Some(first) = keys.first() else {
        return (Vec3::ZERO, Quat::IDENTITY);
    };
    if t <= first.t_sec {
        return (first.position_m, first.rotation);
    }
    let last = keys[keys.len() - 1];
    if t >= last.t_sec {
        return (last.position_m, last.rotation);
    }
    let i = keys.partition_point(|k| k.t_sec <= t) - 1;
    let a = keys[i];
    let b = keys[i + 1];
    let f = (t - a.t_sec) / (b.t_sec - a.t_sec);
    (
        Vec3::lerp(a.position_m, b.position_m, f),
        Quat::slerp(a.rotation, b.rotation, f),
    )
}

fn interp_gaze(keys: &[GazeKey], t: f32) -> (f32, f32) {
    let Some(first) = keys.first() else {
        return (0.0, 0.0);
    };
    if t <= first.t_sec {
        return (first.yaw_deg, first.pitch_deg);
    }
    let last = keys[keys.len() - 1];
    if t >= last.t_sec {
        return (last.yaw_deg, last.pitch_deg);
    }
    let i = keys.partition_point(|k| k.t_sec <= t) - 1;
    let a = keys[i];
    let b = keys[i + 1];
    let f = (t - a.t_sec) / (b.t_sec - a.t_sec);
    (
        a.yaw_deg + (b.yaw_deg - a.yaw_deg) * f,
        a.pitch_deg + (b.pitch_deg - a.pitch_deg) * f,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRAJ_JSON: &str = r#"{
      "version": 1,
      "head": [
        { "t_sec": 0.0, "position_m": [0.0, 1.7, 0.0], "rotation_euler_ypr_deg": [0, 0, 0] },
        { "t_sec": 2.0, "position_m": [0.0, 1.7, -2.0], "rotation_euler_ypr_deg": [90, 0, 0] }
      ],
      "gaze": [
        { "t_sec": 0.0, "yaw_deg": 0.0, "pitch_deg": 0.0 },
        { "t_sec": 2.0, "yaw_deg": 10.0, "pitch_deg": -5.0 }
      ],
      "events": [
        { "t_sec": 1.0, "type": "gesture", "name": "pinch" },
        { "t_sec": 1.5, "type": "gaze_at", "target": "tv" }
      ]
    }"#;

    fn approx(a: f32, b: f32, tol: f32) {
        assert!(
            (a - b).abs() <= tol,
            "approx failed: {a} vs {b} (tol {tol})"
        );
    }

    fn v_approx(a: Vec3, b: Vec3, tol: f32) {
        approx(a.x, b.x, tol);
        approx(a.y, b.y, tol);
        approx(a.z, b.z, tol);
    }

    #[test]
    fn parses_and_samples_keyframes() {
        let t = Trajectory::parse(TRAJ_JSON).unwrap();
        // duration = max(t_sec) across sections
        approx(t.duration_sec(), 2.0, 1e-6);

        let s0 = t.sample(0.0);
        v_approx(s0.camera.position_m, Vec3::new(0.0, 1.7, 0.0), 1e-6);
        v_approx(s0.camera.forward(), Vec3::new(0.0, 0.0, -1.0), 1e-6);

        let s1 = t.sample(1.0);
        v_approx(s1.camera.position_m, Vec3::new(0.0, 1.7, -1.0), 1e-6);
        // Head yaw 45° (slerp midpoint) + gaze yaw 5°, pitch −2.5°.
        v_approx(
            s1.camera.forward(),
            Vec3::new(-0.76553, -0.04362, -0.64218),
            1e-3,
        );

        let s2 = t.sample(2.0);
        v_approx(s2.camera.position_m, Vec3::new(0.0, 1.7, -2.0), 1e-6);
        // Head yaw 90° + gaze yaw 10° → 100°; pitch −5°.
        v_approx(
            s2.camera.forward(),
            Vec3::new(-0.98106, -0.08716, 0.17299),
            1e-3,
        );
    }

    #[test]
    fn clamps_outside_keyframe_range() {
        let t = Trajectory::parse(TRAJ_JSON).unwrap();
        let before = t.sample(-1.0);
        v_approx(before.camera.position_m, Vec3::new(0.0, 1.7, 0.0), 1e-6);
        let after = t.sample(99.0);
        v_approx(after.camera.position_m, Vec3::new(0.0, 1.7, -2.0), 1e-6);
    }

    #[test]
    fn explicit_duration_extends_the_hold() {
        let t = Trajectory::parse(
            r#"{ "head": [ {"t_sec": 0.0, "position_m": [0, 1.7, 0]},
                          {"t_sec": 1.0, "position_m": [0, 1.7, -1]} ],
                "duration_sec": 5.0 }"#,
        )
        .unwrap();
        approx(t.duration_sec(), 5.0, 1e-6);
        let s = t.sample(4.0);
        v_approx(s.camera.position_m, Vec3::new(0.0, 1.7, -1.0), 1e-6);
    }

    #[test]
    fn rotation_quat_encoding_matches_euler() {
        let s = core::f32::consts::FRAC_1_SQRT_2;
        let t = Trajectory::from_value(&serde_json::json!({
            "head": [
                { "t_sec": 0.0, "position_m": [0.0, 1.7, 0.0],
                  "rotation_quat": [0.0, s, 0.0, s] }
            ]
        }))
        .unwrap();
        v_approx(
            t.sample(0.0).camera.forward(),
            Vec3::new(-1.0, 0.0, 0.0),
            1e-5,
        );
    }

    #[test]
    fn omitted_rotation_defaults_to_identity() {
        let t = Trajectory::parse(r#"{ "head": [ {"t_sec": 0.0, "position_m": [0, 1.7, 0]} ] }"#)
            .unwrap();
        v_approx(
            t.sample(0.0).camera.forward(),
            Vec3::new(0.0, 0.0, -1.0),
            1e-6,
        );
    }

    #[test]
    fn missing_gaze_defaults_to_centered() {
        let t = Trajectory::parse(r#"{ "head": [ {"t_sec": 0.0, "position_m": [0, 1.7, 0]} ] }"#)
            .unwrap();
        let s = t.sample(0.0);
        assert_eq!(s.gaze_yaw_deg, 0.0);
        assert_eq!(s.gaze_pitch_deg, 0.0);
        v_approx(s.camera.forward(), Vec3::new(0.0, 0.0, -1.0), 1e-6);
    }

    #[test]
    fn parses_yaml_trajectory() {
        let yaml = "\
version: 1
head:
  - t_sec: 0.0
    position_m: [0.0, 1.7, 0.0]
    rotation_euler_ypr_deg: [0.0, 0.0, 0.0]
  - t_sec: 1.0
    position_m: [0.0, 1.7, -1.0]
gaze:
  - t_sec: 0.0
    yaw_deg: 0.0
    pitch_deg: 0.0
";
        let t = Trajectory::parse(yaml).unwrap();
        approx(t.duration_sec(), 1.0, 1e-6);
        v_approx(
            t.sample(0.5).camera.position_m,
            Vec3::new(0.0, 1.7, -0.5),
            1e-6,
        );
    }

    #[test]
    fn events_between_uses_half_open_interval() {
        let t = Trajectory::parse(TRAJ_JSON).unwrap();
        let early = t.events_between(0.0, 0.99);
        assert!(early.is_empty());
        let mid = t.events_between(0.5, 1.25);
        assert_eq!(mid.len(), 1);
        assert_eq!(mid[0].kind, "gesture");
        let both = t.events_between(1.0, 2.0);
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].kind, "gaze_at");
    }

    // -----------------------------------------------------------------
    // validation codes

    fn err_code(text: &str) -> Issue {
        Trajectory::parse(text).unwrap_err()
    }

    #[test]
    fn t01_head_missing_or_empty() {
        let e = err_code(r#"{"gaze": []}"#);
        assert_eq!(e.code, "T-01");
        assert_eq!(e.path, "/head");
        let e = err_code(r#"{"head": []}"#);
        assert_eq!(e.code, "T-01");
    }

    #[test]
    fn t02_non_increasing_times() {
        let e = err_code(
            r#"{ "head": [ {"t_sec": 1.0, "position_m": [0,0,0]},
                          {"t_sec": 1.0, "position_m": [1,0,0]} ] }"#,
        );
        assert_eq!(e.code, "T-02");
        assert_eq!(e.path, "/head/1/t_sec");
        let e = err_code(
            r#"{ "head": [ {"t_sec": 0.0, "position_m": [0,0,0]} ],
                "gaze": [ {"t_sec": 1.0}, {"t_sec": 0.5} ] }"#,
        );
        assert_eq!(e.code, "T-02");
        assert_eq!(e.path, "/gaze/1/t_sec");
    }

    #[test]
    fn t03_rotation_encodings_conflict_or_invalid() {
        let e = err_code(
            r#"{ "head": [ {"t_sec": 0.0, "position_m": [0,0,0],
                            "rotation_quat": [0,0,0,1],
                            "rotation_euler_ypr_deg": [0,0,0]} ] }"#,
        );
        assert_eq!(e.code, "T-03");
        assert_eq!(e.path, "/head/0");

        let e = err_code(
            r#"{ "head": [ {"t_sec": 0.0, "position_m": [0,0,0],
                            "rotation_quat": [0,0,0,0]} ] }"#,
        );
        assert_eq!(e.code, "T-03");
        assert_eq!(e.path, "/head/0/rotation_quat");

        let e = err_code(
            r#"{ "head": [ {"t_sec": 0.0, "position_m": [0,0,0],
                            "rotation_quat": [0,0,1]} ] }"#,
        );
        assert_eq!(e.code, "T-03");
        assert!(e.path.starts_with("/head/0/rotation_quat"));
    }

    #[test]
    fn t04_missing_or_malformed_numbers() {
        let e = err_code(r#"{ "head": [ {"position_m": [0,0,0]} ] }"#);
        assert_eq!(e.code, "T-04");
        assert_eq!(e.path, "/head/0/t_sec");

        let e = err_code(r#"{ "head": [ {"t_sec": 0.0, "position_m": [0,0]} ] }"#);
        assert_eq!(e.code, "T-04");
        assert_eq!(e.path, "/head/0/position_m");

        let e = err_code(r#"{ "head": [ {"t_sec": "soon", "position_m": [0,0,0]} ] }"#);
        assert_eq!(e.code, "T-04");
        assert_eq!(e.path, "/head/0/t_sec");
    }

    #[test]
    fn t05_event_type_problems() {
        let e = err_code(
            r#"{ "head": [ {"t_sec": 0.0, "position_m": [0,0,0]} ],
                "events": [ {"t_sec": 1.0, "type": "tap"} ] }"#,
        );
        assert_eq!(e.code, "T-05");
        assert_eq!(e.path, "/events/0/type");

        let e = err_code(
            r#"{ "head": [ {"t_sec": 0.0, "position_m": [0,0,0]} ],
                "events": [ {"t_sec": 1.0, "type": "gaze_at"} ] }"#,
        );
        assert_eq!(e.code, "T-05");
        assert_eq!(e.path, "/events/0/target");

        let e = err_code(
            r#"{ "head": [ {"t_sec": 0.0, "position_m": [0,0,0]} ],
                "events": [ {"t_sec": 1.0} ] }"#,
        );
        assert_eq!(e.code, "T-05");
        assert_eq!(e.path, "/events/0/type");
    }

    #[test]
    fn t06_duration_shorter_than_events() {
        let e = err_code(
            r#"{ "head": [ {"t_sec": 0.0, "position_m": [0,0,0]} ],
                "events": [ {"t_sec": 5.0, "type": "gesture", "name": "pinch"} ],
                "duration_sec": 2.0 }"#,
        );
        assert_eq!(e.code, "T-06");
        assert_eq!(e.path, "/duration_sec");
    }

    #[test]
    fn p01_garbage_input() {
        let e = err_code("not: [a, document");
        assert_eq!(e.code, "P-01");
    }
}

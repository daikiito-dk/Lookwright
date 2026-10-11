//! Spatial UI anchors: world-lock / head-lock placement, billboard facing,
//! fault-injection wobble, and visibility sampling (SPEC.md §5, §13).

use crate::camera::CameraPose;
use crate::error::{value_f32, value_vec3, Issue};
use crate::math::{Quat, Vec3};
use serde_json::Value;

/// Anchor mode (SPEC.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorMode {
    World,
    Head,
}

/// Anchor definition. `World` fixes the panel in world space (optionally
/// re-facing the camera when `billboard`); `Head` is HUD-style, placed by the
/// camera pose applied to `offset_m`.
#[derive(Debug, Clone, PartialEq)]
pub enum Anchor {
    World {
        position_m: Vec3,
        rotation_euler_ypr_deg: [f32; 3],
        billboard: bool,
    },
    Head {
        offset_m: Vec3,
    },
}

impl Anchor {
    pub fn mode(&self) -> AnchorMode {
        match self {
            Anchor::World { .. } => AnchorMode::World,
            Anchor::Head { .. } => AnchorMode::Head,
        }
    }
}

/// Fault injection for the `anchor_drift` assertion (SPEC.md §5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wobble {
    pub amplitude_m: f32,
    pub frequency_hz: f32,
}

/// A UI panel definition from a test config.
#[derive(Debug, Clone, PartialEq)]
pub struct UiSpec {
    pub id: String,
    pub size_m: [f32; 2],
    pub anchor: Anchor,
    pub wobble: Option<Wobble>,
}

/// The placed panel for one sample: actual + expected center, facing
/// rotation, quad corners, and the drift fault metric (SPEC.md §5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiPlacement {
    pub center_m: Vec3,
    pub expected_center_m: Vec3,
    pub rotation: Quat,
    /// Ordered [+x+y, −x+y, −x−y, +x−y].
    pub corners_m: [Vec3; 4],
    pub drift_m: f32,
}

impl UiSpec {
    /// Parse one UI element from a config fragment. `path` is the JSON
    /// Pointer prefix (e.g. `/ui/0`) used in coded errors.
    pub fn from_value(v: &Value, path: &str) -> Result<Self, Issue> {
        if !v.is_object() {
            return Err(Issue::new(
                "W-06",
                path,
                format!("UI element must be an object, got {v}"),
            ));
        }
        let id = v
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                Issue::new(
                    "C-02",
                    format!("{path}/id"),
                    format!("UI element requires a non-empty id, got {v}"),
                )
            })?;

        let size_v = v.get("size_m").ok_or_else(|| {
            Issue::new(
                "W-06",
                format!("{path}/size_m"),
                "UI element requires size_m [width_m, height_m]",
            )
        })?;
        let size = crate::error::value_vecn(size_v, &format!("{path}/size_m"), "W-06", 2)?;
        if size[0] <= 0.0 || size[1] <= 0.0 {
            return Err(Issue::new(
                "W-06",
                format!("{path}/size_m"),
                format!("size_m components must be > 0, got {size:?}"),
            ));
        }

        let anchor_v = v.get("anchor").ok_or_else(|| {
            Issue::new(
                "W-06",
                format!("{path}/anchor"),
                "UI element requires an anchor object",
            )
        })?;
        if !anchor_v.is_object() {
            return Err(Issue::new(
                "W-06",
                format!("{path}/anchor"),
                format!("anchor must be an object, got {anchor_v}"),
            ));
        }
        let mode = anchor_v
            .get("mode")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Issue::new(
                    "W-06",
                    format!("{path}/anchor/mode"),
                    format!("anchor requires mode \"world\" or \"head\", got {anchor_v}"),
                )
            })?;
        let anchor = match mode {
            "world" => {
                let position_m = match anchor_v.get("position_m") {
                    Some(p) => value_vec3(p, &format!("{path}/anchor/position_m"), "W-06")?,
                    None => {
                        return Err(Issue::new(
                            "W-06",
                            format!("{path}/anchor/position_m"),
                            "world anchor requires position_m [x, y, z]",
                        ));
                    }
                };
                let rotation_euler_ypr_deg = match anchor_v.get("rotation_euler_ypr_deg") {
                    Some(r) => {
                        let a = crate::error::value_vecn(
                            r,
                            &format!("{path}/anchor/rotation_euler_ypr_deg"),
                            "W-06",
                            3,
                        )?;
                        [a[0], a[1], a[2]]
                    }
                    None => [0.0, 0.0, 0.0],
                };
                let billboard = match anchor_v.get("billboard") {
                    None => false,
                    Some(bv) => bv.as_bool().ok_or_else(|| {
                        Issue::new(
                            "W-06",
                            format!("{path}/anchor/billboard"),
                            format!("billboard must be a boolean, got {bv}"),
                        )
                    })?,
                };
                Anchor::World {
                    position_m,
                    rotation_euler_ypr_deg,
                    billboard,
                }
            }
            "head" => {
                let offset_m = match anchor_v.get("offset_m") {
                    Some(o) => value_vec3(o, &format!("{path}/anchor/offset_m"), "W-06")?,
                    None => {
                        return Err(Issue::new(
                            "W-06",
                            format!("{path}/anchor/offset_m"),
                            "head anchor requires offset_m [x, y, z]",
                        ));
                    }
                };
                Anchor::Head { offset_m }
            }
            other => {
                return Err(Issue::new(
                    "W-06",
                    format!("{path}/anchor/mode"),
                    format!("unknown anchor mode {other:?}; expected \"world\" or \"head\""),
                ));
            }
        };

        let wobble = match v.get("wobble") {
            None => None,
            Some(wv) => {
                if !wv.is_object() {
                    return Err(Issue::new(
                        "W-06",
                        format!("{path}/wobble"),
                        format!("wobble must be an object, got {wv}"),
                    ));
                }
                let amplitude_m = match wv.get("amplitude_m") {
                    Some(a) => value_f32(a, &format!("{path}/wobble/amplitude_m"), "W-06")?,
                    None => {
                        return Err(Issue::new(
                            "W-06",
                            format!("{path}/wobble/amplitude_m"),
                            "wobble requires amplitude_m",
                        ));
                    }
                };
                let frequency_hz = match wv.get("frequency_hz") {
                    Some(f) => value_f32(f, &format!("{path}/wobble/frequency_hz"), "W-06")?,
                    None => {
                        return Err(Issue::new(
                            "W-06",
                            format!("{path}/wobble/frequency_hz"),
                            "wobble requires frequency_hz",
                        ));
                    }
                };
                if amplitude_m < 0.0 || frequency_hz <= 0.0 {
                    return Err(Issue::new(
                        "W-06",
                        format!("{path}/wobble"),
                        format!(
                            "wobble requires amplitude_m >= 0 and frequency_hz > 0, got amplitude_m {amplitude_m}, frequency_hz {frequency_hz}"
                        ),
                    ));
                }
                Some(Wobble {
                    amplitude_m,
                    frequency_hz,
                })
            }
        };

        Ok(Self {
            id: id.to_string(),
            size_m: [size[0], size[1]],
            anchor,
            wobble,
        })
    }

    /// Place the panel at `t_sec` for the given camera pose (SPEC.md §5).
    pub fn place(&self, t_sec: f32, camera: &CameraPose) -> UiPlacement {
        let (expected, rotation) = match &self.anchor {
            Anchor::World {
                position_m,
                rotation_euler_ypr_deg,
                billboard,
            } => {
                let rot = if *billboard {
                    // Face the camera: panel forward (−Z) points at the camera.
                    let fwd = (camera.position_m - *position_m).normalized();
                    let world_up = Vec3::new(0.0, 1.0, 0.0);
                    let ref_up = if fwd.cross(world_up).length() < 1e-5 {
                        Vec3::new(1.0, 0.0, 0.0)
                    } else {
                        world_up
                    };
                    let right = fwd.cross(ref_up).normalized();
                    let up = (fwd * -1.0).cross(right);
                    Quat::from_basis(right, up, fwd)
                } else {
                    Quat::from_euler_ypr_deg(
                        rotation_euler_ypr_deg[0],
                        rotation_euler_ypr_deg[1],
                        rotation_euler_ypr_deg[2],
                    )
                };
                (*position_m, rot)
            }
            Anchor::Head { offset_m } => (
                camera.position_m + camera.rotation.rotate(*offset_m),
                camera.rotation,
            ),
        };

        // Wobble fault injection (SPEC.md §5): the same sinusoid on each axis.
        let actual = match &self.wobble {
            Some(w) => {
                let s =
                    (2.0 * core::f32::consts::PI * w.frequency_hz * t_sec).sin() * w.amplitude_m;
                expected + Vec3::new(s, s, s)
            }
            None => expected,
        };

        let half_w = self.size_m[0] * 0.5;
        let half_h = self.size_m[1] * 0.5;
        let r = rotation.rotate(Vec3::new(1.0, 0.0, 0.0)) * half_w;
        let u = rotation.rotate(Vec3::new(0.0, 1.0, 0.0)) * half_h;
        let corners_m = [
            actual + r + u,
            actual - r + u,
            actual - r - u,
            actual + r - u,
        ];
        UiPlacement {
            center_m: actual,
            expected_center_m: expected,
            rotation,
            corners_m,
            drift_m: (actual - expected).length(),
        }
    }
}

impl UiPlacement {
    /// Sample the quad on a `grid`×`grid` lattice, row-major from the +x+y
    /// corner (SPEC.md §5). `grid >= 2` includes the corners; `grid == 1`
    /// yields the center.
    pub fn grid_points(&self, grid: usize) -> Vec<Vec3> {
        let n = grid.max(1);
        let frac = |i: usize| -> f32 {
            if n == 1 {
                0.5
            } else {
                i as f32 / (n - 1) as f32
            }
        };
        let mut pts = Vec::with_capacity(n * n);
        for j in 0..n {
            let v = frac(j);
            for i in 0..n {
                let u = frac(i);
                let e_top = Vec3::lerp(self.corners_m[0], self.corners_m[1], u);
                let e_bot = Vec3::lerp(self.corners_m[3], self.corners_m[2], u);
                pts.push(Vec3::lerp(e_top, e_bot, v));
            }
        }
        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    fn camera_at_origin() -> CameraPose {
        CameraPose::new(Vec3::ZERO, Quat::IDENTITY)
    }

    #[test]
    fn world_anchor_is_fixed_and_facing_by_euler() {
        let ui = UiSpec::from_value(
            &json!({
                "id": "card",
                "size_m": [0.4, 0.25],
                "anchor": { "mode": "world", "position_m": [1.0, 1.5, -3.0],
                            "rotation_euler_ypr_deg": [90.0, 0.0, 0.0] }
            }),
            "/ui/0",
        )
        .unwrap();
        let p = ui.place(2.0, &camera_at_origin());
        v_approx(p.center_m, Vec3::new(1.0, 1.5, -3.0), 1e-6);
        v_approx(p.expected_center_m, p.center_m, 1e-9);
        assert_eq!(p.drift_m, 0.0);
        // Yaw 90°: panel +X (right) maps to world −Z.
        let right = p.rotation.rotate(Vec3::new(1.0, 0.0, 0.0));
        v_approx(right, Vec3::new(0.0, 0.0, -1.0), 1e-6);
        // Corner +x+y = center + right*0.2 + up*0.125.
        v_approx(p.corners_m[0], Vec3::new(1.0, 1.625, -3.2), 1e-6);
    }

    #[test]
    fn head_anchor_tracks_camera() {
        let ui = UiSpec::from_value(
            &json!({
                "id": "hud",
                "size_m": [0.3, 0.15],
                "anchor": { "mode": "head", "offset_m": [0.0, -0.2, -1.2] }
            }),
            "/ui/0",
        )
        .unwrap();

        let p = ui.place(0.0, &camera_at_origin());
        v_approx(p.center_m, Vec3::new(0.0, -0.2, -1.2), 1e-6);
        // Parallel to the image plane: corners lie in the camera XY plane.
        v_approx(p.corners_m[0], Vec3::new(0.15, -0.125, -1.2), 1e-6);

        let yaw90 = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0);
        let turned = CameraPose::new(Vec3::new(0.5, 1.7, -2.0), yaw90);
        let p = ui.place(3.0, &turned);
        // offset rotated by yaw 90°: (0, −0.2, −1.2) → (−1.2, −0.2, 0).
        v_approx(p.center_m, Vec3::new(0.5 - 1.2, 1.5, -2.0), 1e-5);
        assert_eq!(p.rotation, turned.rotation);
    }

    #[test]
    fn billboard_re_faces_the_camera() {
        let ui = UiSpec::from_value(
            &json!({
                "id": "tag",
                "size_m": [0.2, 0.1],
                "anchor": { "mode": "world", "position_m": [0.0, 1.5, -2.0],
                            "billboard": true }
            }),
            "/ui/0",
        )
        .unwrap();
        let camera = CameraPose::new(Vec3::new(2.0, 1.7, 0.0), Quat::IDENTITY);
        let p = ui.place(0.0, &camera);
        let fwd = (camera.position_m - Vec3::new(0.0, 1.5, -2.0)).normalized();
        v_approx(p.rotation.rotate(Vec3::new(0.0, 0.0, -1.0)), fwd, 1e-5);
        v_approx(p.center_m, Vec3::new(0.0, 1.5, -2.0), 1e-9);
    }

    #[test]
    fn wobble_injects_deterministic_drift() {
        let ui = UiSpec::from_value(
            &json!({
                "id": "bad",
                "size_m": [0.4, 0.25],
                "anchor": { "mode": "world", "position_m": [0.0, 1.5, -2.0] },
                "wobble": { "amplitude_m": 0.1, "frequency_hz": 1.0 }
            }),
            "/ui/0",
        )
        .unwrap();
        let p0 = ui.place(0.0, &camera_at_origin());
        assert_eq!(p0.drift_m, 0.0);
        // t = 0.25 s: sin(π/2) = 1 → offset (0.1, 0.1, 0.1), drift √3·0.1.
        let p = ui.place(0.25, &camera_at_origin());
        v_approx(
            p.center_m - p.expected_center_m,
            Vec3::new(0.1, 0.1, 0.1),
            1e-6,
        );
        approx(p.drift_m, 0.1 * 3.0f32.sqrt(), 1e-6);
    }

    #[test]
    fn grid_points_cover_corners_and_center() {
        let ui = UiSpec::from_value(
            &json!({
                "id": "grid",
                "size_m": [2.0, 2.0],
                "anchor": { "mode": "world", "position_m": [0.0, 0.0, 0.0] }
            }),
            "/ui/0",
        )
        .unwrap();
        let p = ui.place(0.0, &camera_at_origin());
        let pts = p.grid_points(4);
        assert_eq!(pts.len(), 16);
        assert_eq!(pts[0], p.corners_m[0]); // +x+y
        assert_eq!(pts[3], p.corners_m[1]); // −x+y
        assert_eq!(pts[12], p.corners_m[3]); // +x−y
        assert_eq!(pts[15], p.corners_m[2]); // −x−y
        v_approx(pts[5], Vec3::new(1.0 / 3.0, 1.0 / 3.0, 0.0), 1e-6);

        let center = p.grid_points(1);
        assert_eq!(center.len(), 1);
        v_approx(center[0], Vec3::ZERO, 1e-6);
    }

    // -----------------------------------------------------------------
    // validation codes

    fn ui_err(v: Value) -> Issue {
        UiSpec::from_value(&v, "/ui/0").unwrap_err()
    }

    #[test]
    fn c02_empty_id() {
        let e = ui_err(
            json!({ "size_m": [0.1, 0.1], "anchor": { "mode": "head", "offset_m": [0, 0, -1] } }),
        );
        assert_eq!(e.code, "C-02");
        assert_eq!(e.path, "/ui/0/id");
    }

    #[test]
    fn w06_unknown_mode_and_bad_fields() {
        let e = ui_err(json!({
            "id": "x", "size_m": [0.1, 0.1],
            "anchor": { "mode": "floating", "position_m": [0, 0, -1] }
        }));
        assert_eq!(e.code, "W-06");
        assert_eq!(e.path, "/ui/0/anchor/mode");

        let e = ui_err(json!({ "id": "x", "anchor": { "mode": "head", "offset_m": [0, 0, -1] } }));
        assert_eq!(e.code, "W-06");
        assert_eq!(e.path, "/ui/0/size_m");

        let e = ui_err(
            json!({ "id": "x", "size_m": [0.0, 0.1], "anchor": { "mode": "head", "offset_m": [0, 0, -1] } }),
        );
        assert_eq!(e.code, "W-06");
        assert_eq!(e.path, "/ui/0/size_m");

        let e = ui_err(json!({ "id": "x", "size_m": [0.1, 0.1], "anchor": { "mode": "world" } }));
        assert_eq!(e.code, "W-06");
        assert_eq!(e.path, "/ui/0/anchor/position_m");

        let e = ui_err(json!({
            "id": "x", "size_m": [0.1, 0.1],
            "anchor": { "mode": "head", "offset_m": [0, 0, -1] },
            "wobble": { "amplitude_m": 0.1, "frequency_hz": 0.0 }
        }));
        assert_eq!(e.code, "W-06");
        assert_eq!(e.path, "/ui/0/wobble");
    }
}

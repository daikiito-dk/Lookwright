//! Perception feed (SPEC.md §8, §13).
//!
//! One `PerceptionFrame` is the mock input for contextual-AI agents: the
//! camera state, the gaze target, and the frustum-visible objects with their
//! pixel-space bounding boxes. Serialized as plain JSON or JSON-LD with
//! byte-exact field names and deterministic ordering (SPEC.md §1).

use std::cmp::Ordering;

use lookwright_core::{CameraPose, Projector, Vec3};
use serde::Serialize;

use crate::frustum::Frustum;
use crate::gaze::cast_gaze;
use crate::world::World;

/// Schema identifier embedded in every frame (SPEC.md §8).
const SCHEMA: &str = "lookwright/perception-frame@1";

/// Base IRI for the inline JSON-LD `@context` (SPEC.md §8). Lookwright hosts
/// no published vocabulary in v0.1, so terms are anchored under this stable
/// namespace; the term list itself is normative, the IRIs are not.
const JSONLD_BASE: &str = "https://lookwright.dev/schema/perception-frame#";

/// One frustum-visible object entry.
#[derive(Debug, Clone, PartialEq)]
struct VisibleObject {
    id: String,
    /// Stored label, or the id when the stored label is empty (SPEC.md §8).
    label: String,
    /// `|camera_position - AABB center|`, meters.
    distance_m: f32,
    /// `[x0, y0, x1, y1]` pixel bounds of the projected AABB corners; `None`
    /// when any corner fails to project (clip w <= 0, SPEC.md §4).
    bbox_px: Option<[f32; 4]>,
    aabb_min_m: Vec3,
    aabb_max_m: Vec3,
}

/// A captured perception frame (SPEC.md §8).
#[derive(Debug, Clone, PartialEq)]
pub struct PerceptionFrame {
    t_sec: f32,
    camera_position_m: Vec3,
    camera_forward: Vec3,
    gaze_target: Option<String>,
    visible_objects: Vec<VisibleObject>,
}

impl PerceptionFrame {
    /// Capture the frame for one time step: frustum-cull the world, project
    /// bounding boxes, and cast the gaze ray (SPEC.md §8).
    pub fn capture(t_sec: f32, camera: &CameraPose, world: &World, projector: &Projector) -> Self {
        let frustum = Frustum::from_vp(projector.vp_matrix());
        let gaze_target =
            cast_gaze(world, camera.position_m, camera.forward()).map(|hit| hit.object_id);
        let mut visible_objects = Vec::new();
        for object in world.objects() {
            let (aabb_min_m, aabb_max_m) = object.aabb();
            if !frustum.intersects_aabb(aabb_min_m, aabb_max_m) {
                continue;
            }
            let center_m = Vec3::new(
                (aabb_min_m.x + aabb_max_m.x) * 0.5,
                (aabb_min_m.y + aabb_max_m.y) * 0.5,
                (aabb_min_m.z + aabb_max_m.z) * 0.5,
            );
            let stored_label = object.label();
            visible_objects.push(VisibleObject {
                id: object.id().to_string(),
                label: if stored_label.is_empty() {
                    object.id().to_string()
                } else {
                    stored_label.to_string()
                },
                distance_m: (camera.position_m - center_m).length(),
                bbox_px: project_aabb(projector, aabb_min_m, aabb_max_m),
                aabb_min_m,
                aabb_max_m,
            });
        }
        // Sort by distance_m ascending, ties by id ascending: deterministic
        // (SPEC.md §8) and stable against float NaN via a total-order fallback.
        visible_objects.sort_by(|a, b| {
            a.distance_m
                .partial_cmp(&b.distance_m)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        Self {
            t_sec,
            camera_position_m: camera.position_m,
            camera_forward: camera.forward(),
            gaze_target,
            visible_objects,
        }
    }

    /// Pretty-printed JSON (SPEC.md §8). Serialization cannot fail — the
    /// DTO below is plain JSON (finite floats, string keys) — so the
    /// `unwrap_or_default` is a defensive path, never taken in practice.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&FrameJson {
            schema: SCHEMA,
            t_sec: self.t_sec,
            camera: CameraJson {
                position_m: [
                    self.camera_position_m.x,
                    self.camera_position_m.y,
                    self.camera_position_m.z,
                ],
                forward: [
                    self.camera_forward.x,
                    self.camera_forward.y,
                    self.camera_forward.z,
                ],
                gaze_target: self.gaze_target.as_deref(),
            },
            visible_objects: self
                .visible_objects
                .iter()
                .map(|v| VisibleObjectJson {
                    id: &v.id,
                    label: &v.label,
                    distance_m: v.distance_m,
                    bbox_px: v.bbox_px,
                    world_aabb_m: WorldAabbJson {
                        min_m: [v.aabb_min_m.x, v.aabb_min_m.y, v.aabb_min_m.z],
                        max_m: [v.aabb_max_m.x, v.aabb_max_m.y, v.aabb_max_m.z],
                    },
                })
                .collect(),
        })
        .unwrap_or_default()
    }

    /// Pretty-printed JSON-LD (SPEC.md §8): an inline `@context` first, then
    /// the same fields in the same order as [`PerceptionFrame::to_json`],
    /// with `@type` annotations on the frame and on every visible object.
    pub fn to_jsonld(&self) -> String {
        serde_json::to_string_pretty(&FrameJsonld {
            context: JsonldContext {
                perception_frame: format!("{JSONLD_BASE}PerceptionFrame"),
                spatial_object: format!("{JSONLD_BASE}SpatialObject"),
                visible_objects: format!("{JSONLD_BASE}visible_objects"),
                gaze_target: format!("{JSONLD_BASE}gaze_target"),
                bbox_px: format!("{JSONLD_BASE}bbox_px"),
                distance_m: format!("{JSONLD_BASE}distance_m"),
                t_sec: format!("{JSONLD_BASE}t_sec"),
            },
            frame_type: "PerceptionFrame",
            schema: SCHEMA,
            t_sec: self.t_sec,
            camera: CameraJson {
                position_m: [
                    self.camera_position_m.x,
                    self.camera_position_m.y,
                    self.camera_position_m.z,
                ],
                forward: [
                    self.camera_forward.x,
                    self.camera_forward.y,
                    self.camera_forward.z,
                ],
                gaze_target: self.gaze_target.as_deref(),
            },
            visible_objects: self
                .visible_objects
                .iter()
                .map(|v| VisibleObjectJsonld {
                    object_type: "SpatialObject",
                    id: &v.id,
                    label: &v.label,
                    distance_m: v.distance_m,
                    bbox_px: v.bbox_px,
                    world_aabb_m: WorldAabbJson {
                        min_m: [v.aabb_min_m.x, v.aabb_min_m.y, v.aabb_min_m.z],
                        max_m: [v.aabb_max_m.x, v.aabb_max_m.y, v.aabb_max_m.z],
                    },
                })
                .collect(),
        })
        .unwrap_or_default()
    }
}

/// Project the 8 AABB corners and take pixel min/max (SPEC.md §8). `None`
/// when any corner fails to project.
fn project_aabb(projector: &Projector, min_m: Vec3, max_m: Vec3) -> Option<[f32; 4]> {
    let mut bounds: Option<[f32; 4]> = None;
    for i in 0..8u8 {
        let corner = Vec3::new(
            if i & 1 != 0 { max_m.x } else { min_m.x },
            if i & 2 != 0 { max_m.y } else { min_m.y },
            if i & 4 != 0 { max_m.z } else { min_m.z },
        );
        let [px, py] = projector.project(corner)?;
        bounds = Some(match bounds {
            None => [px, py, px, py],
            Some([x0, y0, x1, y1]) => [x0.min(px), y0.min(py), x1.max(px), y1.max(py)],
        });
    }
    bounds
}

// Serialization DTOs: field order is normative (SPEC.md §8) — serde emits
// struct fields in declaration order, and none of these types use maps, so
// the output needs no sorted-key fallback (SPEC.md §1 determinism).

#[derive(Serialize)]
struct CameraJson<'a> {
    position_m: [f32; 3],
    forward: [f32; 3],
    gaze_target: Option<&'a str>,
}

#[derive(Serialize)]
struct WorldAabbJson {
    min_m: [f32; 3],
    max_m: [f32; 3],
}

#[derive(Serialize)]
struct VisibleObjectJson<'a> {
    id: &'a str,
    label: &'a str,
    distance_m: f32,
    bbox_px: Option<[f32; 4]>,
    world_aabb_m: WorldAabbJson,
}

#[derive(Serialize)]
struct FrameJson<'a> {
    schema: &'a str,
    t_sec: f32,
    camera: CameraJson<'a>,
    visible_objects: Vec<VisibleObjectJson<'a>>,
}

#[derive(Serialize)]
struct JsonldContext {
    #[serde(rename = "PerceptionFrame")]
    perception_frame: String,
    #[serde(rename = "SpatialObject")]
    spatial_object: String,
    visible_objects: String,
    gaze_target: String,
    bbox_px: String,
    distance_m: String,
    t_sec: String,
}

#[derive(Serialize)]
struct VisibleObjectJsonld<'a> {
    #[serde(rename = "@type")]
    object_type: &'a str,
    id: &'a str,
    label: &'a str,
    distance_m: f32,
    bbox_px: Option<[f32; 4]>,
    world_aabb_m: WorldAabbJson,
}

#[derive(Serialize)]
struct FrameJsonld<'a> {
    #[serde(rename = "@context")]
    context: JsonldContext,
    #[serde(rename = "@type")]
    frame_type: &'a str,
    schema: &'a str,
    t_sec: f32,
    camera: CameraJson<'a>,
    visible_objects: Vec<VisibleObjectJsonld<'a>>,
}

// ---------------------------------------------------------------------------
// tests — hand-computed expectations only

#[cfg(test)]
mod tests {
    use super::*;
    use crate::World;
    use lookwright_core::{CameraParams, Quat};
    use serde_json::Value;

    fn approx(actual: f32, expected: f32, tol: f32) {
        assert!(
            (actual - expected).abs() <= tol,
            "approx failed: {actual} vs {expected} (tol {tol})"
        );
    }

    fn living_room_frame() -> PerceptionFrame {
        let world = World::from_preset("living_room").unwrap();
        let camera = CameraPose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
        let params = CameraParams::new(70.0, 0.05, 50.0, 1.6);
        let projector = Projector::new(&camera, &params, [640, 400]);
        PerceptionFrame::capture(2.5, &camera, &world, &projector)
    }

    fn visible<'a>(frame_json: &'a Value, id: &str) -> Option<&'a Value> {
        frame_json["visible_objects"]
            .as_array()?
            .iter()
            .find(|entry| entry["id"] == id)
    }

    #[test]
    fn frame_serializes_the_spec_schema() {
        let json = living_room_frame().to_json();
        // Byte-level field order: `schema` is the first key (SPEC.md §8).
        assert!(json.starts_with("{\n  \"schema\""));
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema"], "lookwright/perception-frame@1");
        assert_eq!(v["t_sec"], serde_json::json!(2.5));
        assert_eq!(
            v["camera"]["position_m"],
            serde_json::json!([0.0, 1.7, 0.0])
        );
        assert_eq!(v["camera"]["forward"], serde_json::json!([0.0, 0.0, -1.0]));
        assert_eq!(v["camera"]["gaze_target"], "tv");
        assert!(v["visible_objects"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn frame_sorts_visible_objects_by_distance_then_id() {
        let v: Value = serde_json::from_str(&living_room_frame().to_json()).unwrap();
        let entries = v["visible_objects"].as_array().unwrap();
        for pair in entries.windows(2) {
            let d0 = pair[0]["distance_m"].as_f64().unwrap();
            let d1 = pair[1]["distance_m"].as_f64().unwrap();
            assert!(
                d0 <= d1 + 1e-9,
                "visible_objects not sorted: {d0} then {d1}"
            );
            if (d0 - d1).abs() <= 1e-9 {
                let id0 = pair[0]["id"].as_str().unwrap();
                let id1 = pair[1]["id"].as_str().unwrap();
                assert!(id0 < id1, "tie not broken by id: {id0} vs {id1}");
            }
        }
    }

    #[test]
    fn tv_projects_into_the_viewport() {
        let v: Value = serde_json::from_str(&living_room_frame().to_json()).unwrap();
        let tv = visible(&v, "tv").expect("tv must be visible from the sofa");
        assert_eq!(tv["label"], "TV");
        let bbox = tv["bbox_px"].as_array().expect("tv bbox_px is null");
        let [x0, y0, x1, y1] = [
            bbox[0].as_f64().unwrap(),
            bbox[1].as_f64().unwrap(),
            bbox[2].as_f64().unwrap(),
            bbox[3].as_f64().unwrap(),
        ];
        assert!((0.0..=640.0).contains(&x0), "x0 {x0}");
        assert!((0.0..=400.0).contains(&y0), "y0 {y0}");
        assert!((0.0..=640.0).contains(&x1), "x1 {x1}");
        assert!((0.0..=400.0).contains(&y1), "y1 {y1}");
        assert!(x0 <= x1 && y0 <= y1);
        // Hand-computed: f = 1/tan(35 deg), fx = f/1.6; the x = +/-0.8 m
        // corners at z = -4.86 give ndc_x = +/-0.14693 -> x in [272.98,
        // 367.02] px; the y = 0.85..1.75 m corners map to y in [197.06,
        // 249.96] px.
        approx(bbox[0].as_f64().unwrap() as f32, 272.98, 0.5);
        approx(bbox[1].as_f64().unwrap() as f32, 197.06, 0.5);
        approx(bbox[2].as_f64().unwrap() as f32, 367.02, 0.5);
        approx(bbox[3].as_f64().unwrap() as f32, 249.96, 0.5);
        let aabb_min = tv["world_aabb_m"]["min_m"].as_array().unwrap();
        approx(aabb_min[0].as_f64().unwrap() as f32, -0.8, 1e-5);
        approx(aabb_min[2].as_f64().unwrap() as f32, -4.94, 1e-5);
    }

    #[test]
    fn jsonld_leads_with_context_and_types_every_object() {
        let frame = living_room_frame();
        let jsonld = frame.to_jsonld();
        assert!(
            jsonld.starts_with("{\n  \"@context\""),
            "@context must be the first key"
        );
        assert!(jsonld.contains("\"@type\": \"PerceptionFrame\""));
        assert!(jsonld.contains("\"@type\": \"SpatialObject\""));
        assert!(jsonld.contains("PerceptionFrame"));
        assert!(jsonld.contains("SpatialObject"));
        assert!(jsonld.contains("visible_objects"));
        assert!(jsonld.contains("gaze_target"));
        assert!(jsonld.contains("bbox_px"));
        assert!(jsonld.contains("distance_m"));
        assert!(jsonld.contains("t_sec"));
        // Same payload as the plain JSON feed.
        let v: Value = serde_json::from_str(&jsonld).unwrap();
        assert_eq!(v["schema"], "lookwright/perception-frame@1");
        assert_eq!(v["camera"]["gaze_target"], "tv");
        assert!(v["visible_objects"].as_array().unwrap().len() >= 3);
        let plain: Value = serde_json::from_str(&frame.to_json()).unwrap();
        assert_eq!(
            v["visible_objects"].as_array().unwrap().len(),
            plain["visible_objects"].as_array().unwrap().len()
        );
    }

    #[test]
    fn camera_enclosing_box_is_visible_with_null_bbox() {
        let world = World::parse(
            r#"{ "version": 1, "name": "Wrap", "objects": [
                { "id": "room_shell", "geometry": { "box": {
                    "center_m": [0.0, 1.7, 0.0], "size_m": [40.0, 40.0, 40.0] } } },
                { "id": "tv", "label": "TV", "geometry": { "box": {
                    "center_m": [0.0, 1.3, -4.9], "size_m": [1.6, 0.9, 0.08] } } }
            ] }"#,
        )
        .unwrap();
        let camera = CameraPose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
        let params = CameraParams::new(70.0, 0.05, 50.0, 1.6);
        let projector = Projector::new(&camera, &params, [640, 400]);
        let frame = PerceptionFrame::capture(0.0, &camera, &world, &projector);
        let v: Value = serde_json::from_str(&frame.to_json()).unwrap();
        let shell = visible(&v, "room_shell").expect("enclosing box must be visible");
        assert!(shell["bbox_px"].is_null(), "bbox_px must be null");
        assert_eq!(v["camera"]["gaze_target"], "tv");
        // Its AABB still round-trips.
        let min_z = shell["world_aabb_m"]["min_m"][2].as_f64().unwrap();
        approx(min_z as f32, -20.0, 1e-5);
    }

    #[test]
    fn frame_on_empty_world_has_no_visible_objects() {
        let world = World::parse(r#"{ "version": 1, "name": "Empty" }"#).unwrap();
        let camera = CameraPose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
        let params = CameraParams::new(70.0, 0.05, 50.0, 1.6);
        let projector = Projector::new(&camera, &params, [640, 400]);
        let frame = PerceptionFrame::capture(1.0, &camera, &world, &projector);
        let v: Value = serde_json::from_str(&frame.to_json()).unwrap();
        assert_eq!(v["visible_objects"], serde_json::json!([]));
        assert!(v["camera"]["gaze_target"].is_null());
    }

    #[test]
    fn label_falls_back_to_the_id() {
        let world = World::parse(
            r#"{ "objects": [ { "id": "bench", "geometry": { "box": {
                 "center_m": [0.0, 0.0, -2.0], "size_m": [1.0, 0.5, 0.5] } } } ] }"#,
        )
        .unwrap();
        let camera = CameraPose::new(Vec3::ZERO, Quat::IDENTITY);
        let params = CameraParams::new(70.0, 0.05, 50.0, 1.6);
        let projector = Projector::new(&camera, &params, [64, 64]);
        let frame = PerceptionFrame::capture(0.0, &camera, &world, &projector);
        let v: Value = serde_json::from_str(&frame.to_json()).unwrap();
        assert_eq!(v["visible_objects"][0]["label"], "bench");
    }
}

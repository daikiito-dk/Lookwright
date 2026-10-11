//! Gaze raycasting (SPEC.md §3.2, §8 `gaze_target`).
//!
//! `cast_gaze` casts one ray from the camera position along its forward
//! vector and returns the nearest world hit. Results are fully deterministic:
//! iteration order is the world's object order, and ties are broken by
//! strict `<`, so the first object always wins an exact tie.

use lookwright_core::Vec3;

use crate::world::World;

/// The nearest object hit by a gaze ray.
#[derive(Debug, Clone, PartialEq)]
pub struct GazeHit {
    /// Id of the hit object.
    pub object_id: String,
    /// Distance from the ray origin along `dir`, in meters.
    pub distance_m: f32,
    /// World-space hit point (origin + dir * distance_m).
    pub point_m: Vec3,
}

/// Cast a gaze ray and return the nearest hit, or `None` when nothing is in
/// view. `dir` is assumed normalized by callers (`CameraPose::forward` is a
/// unit vector); a zero direction never hits anything.
pub fn cast_gaze(world: &World, origin_m: Vec3, dir: Vec3) -> Option<GazeHit> {
    if dir.length() == 0.0 {
        return None;
    }
    let mut best: Option<(usize, f32)> = None;
    for (index, object) in world.objects().iter().enumerate() {
        if let Some(t) = object.raycast(origin_m, dir) {
            // Strict `<`: exact ties keep the first (lowest-index) object.
            if t < best.map_or(f32::INFINITY, |(_, current)| current) {
                best = Some((index, t));
            }
        }
    }
    best.map(|(index, distance_m)| GazeHit {
        object_id: world.objects()[index].id().to_string(),
        distance_m,
        point_m: origin_m + dir * distance_m,
    })
}

// ---------------------------------------------------------------------------
// tests — hand-computed expectations only

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Geometry;
    use crate::World;
    use lookwright_core::Quat;

    fn approx(actual: f32, expected: f32, tol: f32) {
        assert!(
            (actual - expected).abs() <= tol,
            "approx failed: {actual} vs {expected} (tol {tol})"
        );
    }

    fn parse_world(json: &str) -> World {
        World::parse(json).unwrap()
    }

    const TWO_BOXES: &str = r#"{
      "objects": [
        { "id": "near", "geometry": { "box": { "center_m": [0.0, 0.0, -3.0], "size_m": [1.0, 1.0, 1.0] } } },
        { "id": "far", "geometry": { "box": { "center_m": [0.0, 0.0, -6.0], "size_m": [1.0, 1.0, 1.0] } } }
      ]
    }"#;

    #[test]
    fn gaze_hits_the_nearest_box() {
        let world = parse_world(TWO_BOXES);
        let hit = cast_gaze(&world, Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0)).unwrap();
        assert_eq!(hit.object_id, "near");
        approx(hit.distance_m, 2.5, 1e-6);
        approx(hit.point_m.z, -2.5, 1e-6);
    }

    #[test]
    fn gaze_miss_returns_none() {
        let world = parse_world(TWO_BOXES);
        let hit = cast_gaze(&world, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        assert!(hit.is_none());
    }

    #[test]
    fn gaze_zero_direction_returns_none() {
        let world = parse_world(TWO_BOXES);
        assert!(cast_gaze(&world, Vec3::ZERO, Vec3::ZERO).is_none());
    }

    #[test]
    fn gaze_hits_points_object_via_its_aabb() {
        let world = parse_world(
            r#"{ "objects": [ { "id": "cloud",
                "geometry": { "points_m": [[0.0, 0.0, -2.0], [1.0, 1.0, -2.0]] } } ] }"#,
        );
        let hit = cast_gaze(&world, Vec3::new(0.5, 0.5, 0.0), Vec3::new(0.0, 0.0, -1.0)).unwrap();
        assert_eq!(hit.object_id, "cloud");
        approx(hit.distance_m, 2.0, 1e-6);
    }

    #[test]
    fn gaze_hits_rotated_box() {
        let world = parse_world(
            r#"{ "objects": [ { "id": "rotated", "geometry": { "box": {
                 "center_m": [0.0, 0.0, 0.0], "size_m": [2.0, 1.0, 1.0],
                 "rotation_euler_ypr_deg": [90.0, 0.0, 0.0] } } } ] }"#,
        );
        // Yaw 90 deg puts the 2 m extent along world z: the +z face is 1 m
        // closer than an unrotated box's +z face would be.
        let hit = cast_gaze(&world, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0)).unwrap();
        assert_eq!(hit.object_id, "rotated");
        approx(hit.distance_m, 4.0, 1e-6);
    }

    #[test]
    fn gaze_inside_an_object_returns_none() {
        let world = parse_world(
            r#"{ "objects": [ { "id": "shell", "geometry": { "box": {
                 "center_m": [0.0, 0.0, 0.0], "size_m": [10.0, 10.0, 10.0] } } } ] }"#,
        );
        assert!(cast_gaze(&world, Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0)).is_none());
    }

    #[test]
    fn gaze_tie_keeps_the_first_object() {
        let world = parse_world(
            r#"{ "objects": [
                { "id": "aaa", "geometry": { "box": { "center_m": [0.0, 0.0, -3.0], "size_m": [1.0, 1.0, 1.0] } } },
                { "id": "bbb", "geometry": { "box": { "center_m": [0.0, 0.0, -3.0], "size_m": [1.0, 1.0, 1.0] } } }
            ] }"#,
        );
        let hit = cast_gaze(&world, Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0)).unwrap();
        assert_eq!(hit.object_id, "aaa");
    }

    #[test]
    fn gaze_hits_mesh_triangles() {
        let triangles = vec![
            [
                Vec3::new(-1.0, -1.0, -2.0),
                Vec3::new(1.0, -1.0, -2.0),
                Vec3::new(0.0, 1.0, -2.0),
            ],
            [
                Vec3::new(-1.0, -1.0, -5.0),
                Vec3::new(1.0, -1.0, -5.0),
                Vec3::new(0.0, 1.0, -5.0),
            ],
        ];
        let object = crate::WorldObject::new(
            "mesh".to_string(),
            String::new(),
            None,
            Geometry::Mesh { triangles },
        );
        let world = World::new(String::new(), vec![object]);
        let hit = cast_gaze(&world, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0)).unwrap();
        assert_eq!(hit.object_id, "mesh");
        approx(hit.distance_m, 2.0, 1e-6);
    }

    // The two CLI load-bearing examples (SPEC.md §2 trajectory examples walk
    // the living room and look at the tv and the painting).
    #[test]
    fn gaze_from_sofa_hits_tv() {
        let world = World::from_preset("living_room").unwrap();
        let camera = lookwright_core::CameraPose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
        let hit = cast_gaze(&world, camera.position_m, camera.forward()).unwrap();
        assert_eq!(hit.object_id, "tv");
        approx(hit.distance_m, 4.86, 1e-4);
    }

    #[test]
    fn gaze_yawed_85_deg_hits_painting() {
        let world = World::from_preset("living_room").unwrap();
        let rotation = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 85.0);
        let camera = lookwright_core::CameraPose::new(Vec3::new(0.0, 1.7, -1.5), rotation);
        let hit = cast_gaze(&world, camera.position_m, camera.forward()).unwrap();
        assert_eq!(hit.object_id, "painting");
    }
}

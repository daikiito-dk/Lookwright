//! Ray intersection primitives (SPEC.md §3.2 raycast dispatch).
//!
//! Private helpers shared by `WorldObject::raycast` and `cast_gaze`. Every
//! test returns the entry distance `t > 1e-6` along the ray, or `None`.
//! `dir` is assumed normalized by callers; the zero vector never hits.

use lookwright_core::{Quat, Vec3};

/// Minimum accepted hit distance; also the parallel/inside rejection margin.
const EPSILON: f32 = 1e-6;

/// Ray vs. axis-aligned box, slab test. Returns the entry distance or `None`
/// when the box is missed, lies behind the ray, or the ray starts inside it.
pub(crate) fn ray_aabb(origin_m: Vec3, dir: Vec3, min_m: Vec3, max_m: Vec3) -> Option<f32> {
    let o = [origin_m.x, origin_m.y, origin_m.z];
    let d = [dir.x, dir.y, dir.z];
    let mn = [min_m.x, min_m.y, min_m.z];
    let mx = [max_m.x, max_m.y, max_m.z];
    let mut t_enter = f32::NEG_INFINITY;
    let mut t_exit = f32::INFINITY;
    for axis in 0..3 {
        if d[axis] == 0.0 {
            // Parallel to this slab: the origin must lie inside it.
            if o[axis] < mn[axis] || o[axis] > mx[axis] {
                return None;
            }
        } else {
            let mut t1 = (mn[axis] - o[axis]) / d[axis];
            let mut t2 = (mx[axis] - o[axis]) / d[axis];
            if t1 > t2 {
                core::mem::swap(&mut t1, &mut t2);
            }
            t_enter = t_enter.max(t1);
            t_exit = t_exit.min(t2);
        }
    }
    if t_enter > t_exit || t_enter <= EPSILON {
        None
    } else {
        Some(t_enter)
    }
}

/// Ray vs. oriented box (SPEC.md §3 `box` with rotation): rotate the ray by
/// the conjugate of the box rotation into local space and run the slab test.
/// The transform is rigid, so distances along the ray are unchanged.
pub(crate) fn ray_obb(
    origin_m: Vec3,
    dir: Vec3,
    center_m: Vec3,
    half_m: Vec3,
    rotation: Quat,
) -> Option<f32> {
    let inv = rotation.conjugate();
    let local_origin = inv.rotate(origin_m - center_m);
    let local_dir = inv.rotate(dir);
    ray_aabb(local_origin, local_dir, half_m * -1.0, half_m)
}

/// Ray vs. single triangle, Möller–Trumbore. No backface culling.
pub(crate) fn ray_triangle(origin_m: Vec3, dir: Vec3, tri: &[Vec3; 3]) -> Option<f32> {
    let edge1 = tri[1] - tri[0];
    let edge2 = tri[2] - tri[0];
    let h = dir.cross(edge2);
    let a = edge1.dot(h);
    if a > -EPSILON && a < EPSILON {
        return None; // Parallel to the triangle plane.
    }
    let f = 1.0 / a;
    let s = origin_m - tri[0];
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(edge1);
    let v = f * dir.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = f * edge2.dot(q);
    if t > EPSILON {
        Some(t)
    } else {
        None
    }
}

/// Nearest triangle hit of a triangle soup (glTF meshes).
pub(crate) fn ray_triangles(origin_m: Vec3, dir: Vec3, triangles: &[[Vec3; 3]]) -> Option<f32> {
    let mut best: Option<f32> = None;
    for tri in triangles {
        if let Some(t) = ray_triangle(origin_m, dir, tri) {
            best = Some(match best {
                None => t,
                Some(b) if t < b => t,
                Some(b) => b,
            });
        }
    }
    best
}

// ---------------------------------------------------------------------------
// tests — hand-computed expectations only

#[cfg(test)]
mod tests {
    use super::*;
    use lookwright_core::Quat;

    fn approx(actual: f32, expected: f32, tol: f32) {
        assert!(
            (actual - expected).abs() <= tol,
            "approx failed: {actual} vs {expected} (tol {tol})"
        );
    }

    const MIN: Vec3 = Vec3::new(-0.5, -0.5, -0.5);
    const MAX: Vec3 = Vec3::new(0.5, 0.5, 0.5);

    #[test]
    fn aabb_hit_from_five_meters_out() {
        // Ray from (0,0,5) toward -z enters the unit box at z = 0.5 -> t = 4.5.
        let t = ray_aabb(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -1.0),
            MIN,
            MAX,
        );
        approx(t.unwrap(), 4.5, 1e-6);
    }

    #[test]
    fn aabb_miss_when_parallel_outside_slab() {
        // Parallel to the x slab and outside it.
        let t = ray_aabb(
            Vec3::new(3.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -1.0),
            MIN,
            MAX,
        );
        assert!(t.is_none());
    }

    #[test]
    fn aabb_miss_when_box_behind_origin() {
        let t = ray_aabb(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 1.0), MIN, MAX);
        assert!(t.is_none());
    }

    #[test]
    fn aabb_returns_none_when_origin_inside() {
        let t = ray_aabb(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), MIN, MAX);
        assert!(t.is_none());
    }

    #[test]
    fn obb_rotates_ray_into_local_space() {
        // Identity rotation behaves like the AABB test.
        let t = ray_obb(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::ZERO,
            Vec3::new(0.5, 0.5, 0.5),
            Quat::IDENTITY,
        );
        approx(t.unwrap(), 4.5, 1e-6);

        // A 90-degree yaw maps the box's local +x face to world -z, so the
        // 2 m local x extent (half = 1.0) becomes the world z extent and the
        // ray enters at z = 1.0 -> t = 4.0.
        let yaw90 = Quat::from_euler_ypr_deg(90.0, 0.0, 0.0);
        let t = ray_obb(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::ZERO,
            Vec3::new(1.0, 0.5, 0.5),
            yaw90,
        );
        approx(t.unwrap(), 4.0, 1e-6);

        // Off-center box: entry is measured from the ray origin.
        let t = ray_obb(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 2.5),
            Vec3::new(0.5, 0.5, 0.5),
            Quat::IDENTITY,
        );
        approx(t.unwrap(), 2.0, 1e-6);
    }

    #[test]
    fn obb_misses_rotated_away() {
        // Same yaw-90 box, but the ray passes beside the world x extent.
        let yaw90 = Quat::from_euler_ypr_deg(90.0, 0.0, 0.0);
        let t = ray_obb(
            Vec3::new(0.75, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::ZERO,
            Vec3::new(1.0, 0.5, 0.5),
            yaw90,
        );
        assert!(t.is_none());
    }

    #[test]
    fn triangle_hit_at_hand_computed_t() {
        let tri = [
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        // (0,0,0) has barycentric u = 0.25, v = 0.5 inside the triangle; the
        // plane z = 0 is 5 m in front of the origin -> t = 5.
        let t = ray_triangle(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0), &tri);
        approx(t.unwrap(), 5.0, 1e-6);
    }

    #[test]
    fn triangle_misses_outside_and_parallel_and_behind() {
        let tri = [
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        // Outside the triangle: x = 0.9 lies beyond the u + v = 1 edge.
        assert!(ray_triangle(Vec3::new(0.9, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0), &tri).is_none());
        // Parallel to the plane.
        assert!(ray_triangle(Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.0, 0.0, 0.0), &tri).is_none());
        // Plane behind the ray origin.
        assert!(ray_triangle(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, -1.0), &tri).is_none());
        // Degenerate (zero-area) triangle never hits.
        let degenerate = [Vec3::ZERO, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)];
        assert!(ray_triangle(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -1.0),
            &degenerate
        )
        .is_none());
    }

    #[test]
    fn triangles_keep_the_nearest_hit() {
        let near = [
            Vec3::new(-1.0, -1.0, -1.0),
            Vec3::new(1.0, -1.0, -1.0),
            Vec3::new(0.0, 1.0, -1.0),
        ];
        let far = [
            Vec3::new(-1.0, -1.0, -4.0),
            Vec3::new(1.0, -1.0, -4.0),
            Vec3::new(0.0, 1.0, -4.0),
        ];
        let t = ray_triangles(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, -1.0),
            &[far, near],
        );
        approx(t.unwrap(), 6.0, 1e-6);
    }
}

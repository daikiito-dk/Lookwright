//! View-frustum culling (SPEC.md §8 `visible_objects`).
//!
//! Gribb–Hartmann plane extraction from the view-projection matrix, followed
//! by the p-vertex AABB test. The test is conservative (it never rejects a
//! box that intersects the frustum) which is exactly the semantics the
//! perception feed wants: "AABB intersects the view frustum".

use lookwright_core::{Mat4, Vec3};

/// Six normalized planes `(a, b, c, d)` in the order left, right, bottom,
/// top, near, far. A world point `p` is inside the frustum iff
/// `a·x + b·y + c·z + d >= 0` for every plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frustum {
    planes: [[f32; 4]; 6],
}

impl Frustum {
    /// Extract the frustum planes from a combined view-projection matrix.
    ///
    /// Clip space (column vector convention, SPEC.md §4) keeps a point inside
    /// while `-w <= x,y,z <= w`. With the core `Mat4` stored column-major as
    /// `m[col * 4 + row]`, clip row `r` of the combined matrix is
    /// `(m[r], m[4 + r], m[8 + r], m[12 + r])`, so each plane is the sum or
    /// difference of clip rows 0..=2 and 3:
    ///
    /// * left   = row3 + row0, right  = row3 - row0
    /// * bottom = row3 + row1, top    = row3 - row1
    /// * near   = row3 + row2, far    = row3 - row2
    pub fn from_vp(vp: Mat4) -> Self {
        let m = &vp.m;
        let mut planes = [[0.0f32; 4]; 6];
        for (plane_index, plane) in planes.iter_mut().enumerate() {
            let clip_row = plane_index / 2; // rows 0, 1, 2 -> x, y, z clip
            let sign = if plane_index % 2 == 0 { 1.0 } else { -1.0 };
            let mut a = m[3] + sign * m[clip_row];
            let mut b = m[7] + sign * m[4 + clip_row];
            let mut c = m[11] + sign * m[8 + clip_row];
            let d = m[15] + sign * m[12 + clip_row];
            let len = (a * a + b * b + c * c).sqrt();
            if len > 0.0 {
                a /= len;
                b /= len;
                c /= len;
                *plane = [a, b, c, d / len];
            } else {
                // Degenerate plane (cannot happen for a valid projection);
                // keep it pass-through so culling never spuriously rejects.
                *plane = [0.0, 0.0, 0.0, 0.0];
            }
        }
        Self { planes }
    }

    /// Conservative AABB-vs-frustum test (p-vertex): the box is outside iff
    /// the corner furthest along some plane normal lies behind that plane.
    pub fn intersects_aabb(&self, min_m: Vec3, max_m: Vec3) -> bool {
        for [a, b, c, d] in &self.planes {
            let px = if *a >= 0.0 { max_m.x } else { min_m.x };
            let py = if *b >= 0.0 { max_m.y } else { min_m.y };
            let pz = if *c >= 0.0 { max_m.z } else { min_m.z };
            if a * px + b * py + c * pz + d < 0.0 {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// tests — hand-computed expectations only

#[cfg(test)]
mod tests {
    use super::*;
    use lookwright_core::{CameraParams, CameraPose, Projector, Quat};

    fn identity_frustum() -> Frustum {
        let camera = CameraPose::new(Vec3::ZERO, Quat::IDENTITY);
        // fov 90 deg, aspect 1: the visible wedge at depth |z| = 5 spans
        // |x| <= 5 and |y| <= 5.
        let params = CameraParams::new(90.0, 0.05, 50.0, 1.0);
        let projector = Projector::new(&camera, &params, [64, 64]);
        Frustum::from_vp(projector.vp_matrix())
    }

    fn box_at(center_m: Vec3, size_m: Vec3) -> (Vec3, Vec3) {
        let half = Vec3::new(size_m.x * 0.5, size_m.y * 0.5, size_m.z * 0.5);
        (center_m - half, center_m + half)
    }

    #[test]
    fn box_in_front_is_visible() {
        let frustum = identity_frustum();
        let (min_m, max_m) = box_at(Vec3::new(0.0, 0.0, -5.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(frustum.intersects_aabb(min_m, max_m));
    }

    #[test]
    fn box_behind_the_camera_is_not_visible() {
        let frustum = identity_frustum();
        let (min_m, max_m) = box_at(Vec3::new(0.0, 0.0, 5.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(!frustum.intersects_aabb(min_m, max_m));
    }

    #[test]
    fn box_far_to_the_right_is_not_visible() {
        let frustum = identity_frustum();
        let (min_m, max_m) = box_at(Vec3::new(10.0, 0.0, -5.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(!frustum.intersects_aabb(min_m, max_m));
    }

    #[test]
    fn box_straddling_the_left_edge_is_visible() {
        let frustum = identity_frustum();
        // The frustum's left boundary at depth z = -5 is x = -5; the box
        // spans x in [-5.5, -4.5] there.
        let (min_m, max_m) = box_at(Vec3::new(-5.0, 0.0, -5.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(frustum.intersects_aabb(min_m, max_m));
    }

    #[test]
    fn huge_box_wrapping_the_camera_is_visible() {
        let frustum = identity_frustum();
        let (min_m, max_m) = box_at(Vec3::ZERO, Vec3::new(200.0, 200.0, 200.0));
        assert!(frustum.intersects_aabb(min_m, max_m));
    }

    #[test]
    fn frustum_follows_camera_yaw() {
        // Yaw +90 turns the camera toward -x (SPEC.md §1): the box ahead
        // moves from (0,0,-5) to (-5,0,0) in world space.
        let camera = CameraPose::new(
            Vec3::ZERO,
            Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0),
        );
        let params = CameraParams::new(90.0, 0.05, 50.0, 1.0);
        let projector = Projector::new(&camera, &params, [64, 64]);
        let frustum = Frustum::from_vp(projector.vp_matrix());

        let (min_m, max_m) = box_at(Vec3::new(-5.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(frustum.intersects_aabb(min_m, max_m));
        let (min_m, max_m) = box_at(Vec3::new(0.0, 0.0, -5.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(!frustum.intersects_aabb(min_m, max_m));
    }

    #[test]
    fn planes_are_normalized() {
        let frustum = identity_frustum();
        for [a, b, c, d] in &frustum.planes {
            // Compare against 1.0 with a tolerance: normalization uses sqrt.
            let len = (a * a + b * b + c * c).sqrt();
            assert!((len - 1.0).abs() < 1e-5, "plane normal length {len}");
            assert!(d.is_finite());
        }
    }
}

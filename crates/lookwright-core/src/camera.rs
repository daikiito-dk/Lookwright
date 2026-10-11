//! Camera pose/parameters and screen projection (SPEC.md §4, §13).

use crate::math::{Mat4, Quat, Vec3};

/// Rigid camera pose in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub position_m: Vec3,
    pub rotation: Quat,
}

impl CameraPose {
    pub const fn new(position_m: Vec3, rotation: Quat) -> Self {
        Self {
            position_m,
            rotation,
        }
    }

    /// Camera look direction (local −Z in world space).
    pub fn forward(&self) -> Vec3 {
        self.rotation.rotate(Vec3::new(0.0, 0.0, -1.0))
    }

    pub fn right(&self) -> Vec3 {
        self.rotation.rotate(Vec3::new(1.0, 0.0, 0.0))
    }

    pub fn up(&self) -> Vec3 {
        self.rotation.rotate(Vec3::new(0.0, 1.0, 0.0))
    }

    /// World→camera matrix (SPEC.md §4).
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::view_from_pose(self.position_m, self.rotation)
    }
}

/// Perspective camera parameters (SPEC.md §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraParams {
    pub fov_y_deg: f32,
    pub near_m: f32,
    pub far_m: f32,
    pub aspect: f32,
}

impl CameraParams {
    pub const fn new(fov_y_deg: f32, near_m: f32, far_m: f32, aspect: f32) -> Self {
        Self {
            fov_y_deg,
            near_m,
            far_m,
            aspect,
        }
    }

    pub fn perspective_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(
            self.fov_y_deg.to_radians(),
            self.aspect,
            self.near_m,
            self.far_m,
        )
    }
}

/// View-projection helper: projects world points to pixel coordinates
/// (SPEC.md §4 screen mapping).
pub struct Projector {
    vp: Mat4,
    pub width_px: u32,
    pub height_px: u32,
}

impl Projector {
    pub fn new(camera: &CameraPose, params: &CameraParams, resolution_px: [u32; 2]) -> Self {
        let vp = params.perspective_matrix().mul_mat(camera.view_matrix());
        Self {
            vp,
            width_px: resolution_px[0],
            height_px: resolution_px[1],
        }
    }

    pub fn vp_matrix(&self) -> Mat4 {
        self.vp
    }

    /// Project a world point to `[x_px, y_px]`. `None` when clip `w <= 0`
    /// (behind or exactly on the eye plane, SPEC.md §4).
    pub fn project(&self, p: Vec3) -> Option<[f32; 2]> {
        let p4 = self.vp.transform4([p.x, p.y, p.z, 1.0]);
        if p4[3] <= 0.0 {
            return None;
        }
        let ndc_x = p4[0] / p4[3];
        let ndc_y = p4[1] / p4[3];
        Some([
            (ndc_x * 0.5 + 0.5) * self.width_px as f32,
            (0.5 - ndc_y * 0.5) * self.height_px as f32,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn cam_params() -> CameraParams {
        CameraParams::new(90.0, 0.1, 100.0, 1.0)
    }

    #[test]
    fn identity_camera_basis() {
        let c = CameraPose::new(Vec3::new(0.0, 1.7, 0.0), Quat::IDENTITY);
        assert_eq!(c.forward(), Vec3::new(0.0, 0.0, -1.0));
        assert_eq!(c.right(), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(c.up(), Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn yawed_camera_basis() {
        let yaw90 = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0);
        let c = CameraPose::new(Vec3::ZERO, yaw90);
        v_approx(c.forward(), Vec3::new(-1.0, 0.0, 0.0), 1e-5);
        v_approx(c.right(), Vec3::new(0.0, 0.0, -1.0), 1e-5);
    }

    #[test]
    fn projector_centers_and_edges() {
        let p = Projector::new(
            &CameraPose::new(Vec3::ZERO, Quat::IDENTITY),
            &cam_params(),
            [100, 100],
        );
        let center = p.project(Vec3::new(0.0, 0.0, -1.0)).unwrap();
        approx(center[0], 50.0, 1e-4);
        approx(center[1], 50.0, 1e-4);

        let left = p.project(Vec3::new(-1.0, 0.0, -1.0)).unwrap();
        approx(left[0], 0.0, 1e-4);

        let top = p.project(Vec3::new(0.0, 1.0, -1.0)).unwrap();
        approx(top[1], 0.0, 1e-4);
    }

    #[test]
    fn projector_rejects_points_at_or_behind_eye() {
        let p = Projector::new(
            &CameraPose::new(Vec3::ZERO, Quat::IDENTITY),
            &cam_params(),
            [100, 100],
        );
        assert!(p.project(Vec3::new(0.0, 0.0, 0.5)).is_none()); // behind
        assert!(p.project(Vec3::new(0.0, 0.0, 0.0)).is_none()); // on the eye plane
    }

    #[test]
    fn projector_respects_aspect() {
        // 200x100 (aspect 2): x=1 at z=−1 lands at ndc x = 0.5 → 150px.
        let params = CameraParams::new(90.0, 0.1, 100.0, 2.0);
        let p = Projector::new(
            &CameraPose::new(Vec3::ZERO, Quat::IDENTITY),
            &params,
            [200, 100],
        );
        let out = p.project(Vec3::new(1.0, 0.0, -1.0)).unwrap();
        approx(out[0], 150.0, 1e-3);
    }

    #[test]
    fn projector_combines_view_and_projection() {
        // Camera 5m back from the origin looking forward: the origin projects
        // to the screen center; depth matches the near/far mapping.
        let p = Projector::new(
            &CameraPose::new(Vec3::new(0.0, 0.0, 5.0), Quat::IDENTITY),
            &cam_params(),
            [100, 100],
        );
        let out = p.project(Vec3::new(0.0, 0.0, 0.0)).unwrap();
        approx(out[0], 50.0, 1e-3);
        approx(out[1], 50.0, 1e-3);

        // vp == perspective · view (checked through a transformed point).
        let vp = p.vp_matrix();
        let clip = vp.transform4([0.0, 0.0, 0.0, 1.0]);
        approx(clip[3], 5.0, 1e-5);
    }

    #[test]
    fn projector_follows_yaw() {
        // Camera yawed left 90°: a world point on the −X axis projects center.
        let yaw90 = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0);
        let p = Projector::new(
            &CameraPose::new(Vec3::ZERO, yaw90),
            &cam_params(),
            [100, 100],
        );
        let out = p.project(Vec3::new(-2.0, 0.0, 0.0)).unwrap();
        approx(out[0], 50.0, 1e-3);
        approx(out[1], 50.0, 1e-3);
    }
}

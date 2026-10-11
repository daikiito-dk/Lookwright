//! Scalar math primitives (SPEC.md §1, §13).
//!
//! Hand-rolled and dependency-free on purpose: plain IEEE-754 f32 operations
//! keep text and raster outputs deterministic (SPEC.md §1 "Determinism").
//! Column-major `Mat4` follows the OpenGL conventions used in §4.

use std::ops::{Add, Mul, Neg, Sub};

// ---------------------------------------------------------------------------
// Vec3

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Self) -> Self {
        Self::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// Unit vector; the zero vector normalizes to zero (never NaN).
    pub fn normalized(self) -> Self {
        let l = self.length();
        if l > 0.0 {
            Self::new(self.x / l, self.y / l, self.z / l)
        } else {
            Self::ZERO
        }
    }

    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self::new(
            a.x + (b.x - a.x) * t,
            a.y + (b.y - a.y) * t,
            a.z + (b.z - a.z) * t,
        )
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

// ---------------------------------------------------------------------------
// Quat

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quat {
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    pub fn from_axis_angle_deg(axis: Vec3, deg: f32) -> Self {
        let axis = axis.normalized();
        let half = deg.to_radians() * 0.5;
        let s = half.sin();
        Self {
            x: axis.x * s,
            y: axis.y * s,
            z: axis.z * s,
            w: half.cos(),
        }
    }

    /// `R = Ry(yaw) · Rx(pitch) · Rz(roll)` in degrees (SPEC.md §1).
    pub fn from_euler_ypr_deg(yaw_deg: f32, pitch_deg: f32, roll_deg: f32) -> Self {
        Self::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), yaw_deg)
            * Self::from_axis_angle_deg(Vec3::new(1.0, 0.0, 0.0), pitch_deg)
            * Self::from_axis_angle_deg(Vec3::new(0.0, 0.0, 1.0), roll_deg)
    }

    pub fn dot(a: Self, b: Self) -> f32 {
        a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w
    }

    pub fn normalized(self) -> Self {
        let l = Quat::dot(self, self).sqrt();
        if l > 0.0 {
            Self::new(self.x / l, self.y / l, self.z / l, self.w / l)
        } else {
            Self::IDENTITY
        }
    }

    pub fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    pub fn rotate(self, v: Vec3) -> Vec3 {
        let q = Vec3::new(self.x, self.y, self.z);
        let t = q.cross(v) * 2.0;
        v + t * self.w + q.cross(t)
    }

    /// Shortest-arc SLERP (SPEC.md §2). Falls back to normalized LERP when the
    /// quaternions are nearly parallel (cos > 0.9995) for numerical stability.
    pub fn slerp(a: Self, b: Self, t: f32) -> Self {
        let mut cos = Quat::dot(a, b);
        let mut b = b;
        if cos < 0.0 {
            b = -b;
            cos = -cos;
        }
        if cos > 0.9995 {
            return Self {
                x: a.x + (b.x - a.x) * t,
                y: a.y + (b.y - a.y) * t,
                z: a.z + (b.z - a.z) * t,
                w: a.w + (b.w - a.w) * t,
            }
            .normalized();
        }
        let cos = cos.clamp(-1.0, 1.0);
        let theta = cos.acos();
        let s = theta.sin();
        let wa = ((1.0 - t) * theta).sin() / s;
        let wb = (t * theta).sin() / s;
        Self::new(
            a.x * wa + b.x * wb,
            a.y * wa + b.y * wb,
            a.z * wa + b.z * wb,
            a.w * wa + b.w * wb,
        )
        .normalized()
    }

    /// Rotation whose local axes map +X→`right`, +Y→`up`, −Z→`forward`
    /// (camera convention, SPEC.md §1). Inputs must be orthonormal.
    pub fn from_basis(right: Vec3, up: Vec3, forward: Vec3) -> Self {
        // Matrix columns: local X, Y, Z images. −Z maps to forward, so the
        // local Z column is the negated forward vector.
        Self::from_matrix_cols(right, up, forward * -1.0)
    }

    /// Quaternion from a rotation matrix given by its column images.
    fn from_matrix_cols(c0: Vec3, c1: Vec3, c2: Vec3) -> Self {
        let (m00, m01, m02) = (c0.x, c1.x, c2.x);
        let (m10, m11, m12) = (c0.y, c1.y, c2.y);
        let (m20, m21, m22) = (c0.z, c1.z, c2.z);
        let tr = m00 + m11 + m22;
        if tr > 0.0 {
            let s = (tr + 1.0).sqrt() * 2.0;
            Self::new((m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s)
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            Self::new(0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s)
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            Self::new((m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s)
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            Self::new((m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s)
        }
    }
}

impl Mul for Quat {
    type Output = Quat;

    /// Hamilton product `self · o`; `(self · o).rotate(v)` applies `o` first.
    fn mul(self, o: Quat) -> Quat {
        Quat {
            w: self.w * o.w - (self.x * o.x + self.y * o.y + self.z * o.z),
            x: self.w * o.x + o.w * self.x + (self.y * o.z - self.z * o.y),
            y: self.w * o.y + o.w * self.y + (self.z * o.x - self.x * o.z),
            z: self.w * o.z + o.w * self.z + (self.x * o.y - self.y * o.x),
        }
    }
}

impl Neg for Quat {
    type Output = Quat;
    fn neg(self) -> Quat {
        Quat::new(-self.x, -self.y, -self.z, -self.w)
    }
}

// ---------------------------------------------------------------------------
// Mat4 (column-major: m[col * 4 + row])

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    pub m: [f32; 16],
}

impl Mat4 {
    pub const IDENTITY: Self = Self {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    pub fn from_cols(c0: [f32; 4], c1: [f32; 4], c2: [f32; 4], c3: [f32; 4]) -> Self {
        Self {
            m: [
                c0[0], c0[1], c0[2], c0[3], c1[0], c1[1], c1[2], c1[3], c2[0], c2[1], c2[2], c2[3],
                c3[0], c3[1], c3[2], c3[3],
            ],
        }
    }

    /// Matrix product `self · o` (apply `o` first).
    pub fn mul_mat(self, o: Self) -> Self {
        let a = self.m;
        let b = o.m;
        let mut out = [0.0f32; 16];
        for c in 0..4 {
            for r in 0..4 {
                out[c * 4 + r] = a[r] * b[c * 4]
                    + a[4 + r] * b[c * 4 + 1]
                    + a[8 + r] * b[c * 4 + 2]
                    + a[12 + r] * b[c * 4 + 3];
            }
        }
        Self { m: out }
    }

    /// Transform a homogeneous point (`M · p`).
    pub fn transform4(self, p: [f32; 4]) -> [f32; 4] {
        let m = self.m;
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12] * p[3],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13] * p[3],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14] * p[3],
            m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15] * p[3],
        ]
    }

    pub fn from_translation(t: Vec3) -> Self {
        let mut m = Self::IDENTITY.m;
        m[12] = t.x;
        m[13] = t.y;
        m[14] = t.z;
        Self { m }
    }

    /// Rotation matrix (columns are the rotated basis images).
    pub fn from_quat(q: Quat) -> Self {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let col0 = [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + w * z),
            2.0 * (x * z - w * y),
            0.0,
        ];
        let col1 = [
            2.0 * (x * y - w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + w * x),
            0.0,
        ];
        let col2 = [
            2.0 * (x * z + w * y),
            2.0 * (y * z - w * x),
            1.0 - 2.0 * (x * x + y * y),
            0.0,
        ];
        let col3 = [0.0, 0.0, 0.0, 1.0];
        Self::from_cols(col0, col1, col2, col3)
    }

    /// Inverse of a rigid `T(position) · R(rotation)` transform (world→camera,
    /// SPEC.md §4): rotation transposed, translation negated and rotated back.
    pub fn view_from_pose(position_m: Vec3, rotation: Quat) -> Self {
        let inv_rot = rotation.conjugate();
        let r = Self::from_quat(inv_rot);
        let t = inv_rot.rotate(position_m * -1.0);
        let mut m = r.m;
        m[12] = t.x;
        m[13] = t.y;
        m[14] = t.z;
        Self { m }
    }

    /// OpenGL-style perspective projection, clip z ∈ [−1, 1] (SPEC.md §4).
    pub fn perspective_rh(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> Self {
        let f = 1.0 / (fov_y_rad * 0.5).tan();
        let c2_z = (far + near) / (near - far);
        let c3_z = 2.0 * far * near / (near - far);
        Self::from_cols(
            [f / aspect, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, c2_z, -1.0],
            [0.0, 0.0, c3_z, 0.0],
        )
    }
}

// ---------------------------------------------------------------------------
// tests

#[cfg(test)]
mod tests {
    use super::*;
    use core::f32::consts::FRAC_1_SQRT_2;

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

    fn q_approx(a: Quat, b: Quat, tol: f32) {
        approx(a.x, b.x, tol);
        approx(a.y, b.y, tol);
        approx(a.z, b.z, tol);
        approx(a.w, b.w, tol);
    }

    #[test]
    fn vec3_ops() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(-1.0, 0.5, 2.0);
        v_approx(a + b, Vec3::new(0.0, 2.5, 5.0), 1e-6);
        v_approx(a - b, Vec3::new(2.0, 1.5, 1.0), 1e-6);
        v_approx(a * 2.0, Vec3::new(2.0, 4.0, 6.0), 1e-6);
        v_approx(-a, Vec3::new(-1.0, -2.0, -3.0), 1e-6);
        approx(a.dot(b), 6.0, 1e-6);
        v_approx(a.cross(b), Vec3::new(2.5, -5.0, 2.5), 1e-6);
        v_approx(Vec3::lerp(a, b, 0.25), Vec3::new(0.5, 1.625, 2.75), 1e-6);
        approx(a.normalized().length(), 1.0, 1e-6);
        v_approx(Vec3::ZERO.normalized(), Vec3::ZERO, 1e-6);
    }

    #[test]
    fn quat_identity_rotates_nothing() {
        v_approx(
            Quat::IDENTITY.rotate(Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(1.0, 2.0, 3.0),
            1e-6,
        );
    }

    #[test]
    fn quat_yaw90_rotates_forward_left() {
        let q = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0);
        v_approx(
            q.rotate(Vec3::new(0.0, 0.0, -1.0)),
            Vec3::new(-1.0, 0.0, 0.0),
            1e-6,
        );
    }

    #[test]
    fn quat_pitch90_looks_up_and_roll90_rotates_x_to_y() {
        let pitch = Quat::from_axis_angle_deg(Vec3::new(1.0, 0.0, 0.0), 90.0);
        v_approx(
            pitch.rotate(Vec3::new(0.0, 0.0, -1.0)),
            Vec3::new(0.0, 1.0, 0.0),
            1e-6,
        );
        let roll = Quat::from_axis_angle_deg(Vec3::new(0.0, 0.0, 1.0), 90.0);
        v_approx(
            roll.rotate(Vec3::new(1.0, 0.0, 0.0)),
            Vec3::new(0.0, 1.0, 0.0),
            1e-6,
        );
    }

    #[test]
    fn from_euler_ypr_composes_axis_angles() {
        let e = Quat::from_euler_ypr_deg(90.0, 0.0, 0.0);
        let a = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0);
        q_approx(e, a, 1e-6);

        // R = Ry(90) · Rx(90): yaw applies after pitch.
        let both = Quat::from_euler_ypr_deg(90.0, 90.0, 0.0);
        let composed = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0)
            * Quat::from_axis_angle_deg(Vec3::new(1.0, 0.0, 0.0), 90.0);
        q_approx(both, composed, 1e-6);
        v_approx(
            both.rotate(Vec3::new(0.0, 0.0, -1.0)),
            Vec3::new(0.0, 1.0, 0.0),
            1e-6,
        );
    }

    #[test]
    fn quat_mul_applies_right_factor_first() {
        let qy = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0);
        let qp = Quat::from_axis_angle_deg(Vec3::new(1.0, 0.0, 0.0), 90.0);
        let v = Vec3::new(0.0, 0.0, -1.0);
        v_approx((qy * qp).rotate(v), qy.rotate(qp.rotate(v)), 1e-6);
    }

    #[test]
    fn slerp_hits_endpoints_and_midpoint() {
        let a = Quat::IDENTITY;
        let b = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0);
        q_approx(Quat::slerp(a, b, 0.0), a, 1e-5);
        q_approx(Quat::slerp(a, b, 1.0), b, 1e-5);
        let mid = Quat::slerp(a, b, 0.5);
        v_approx(
            mid.rotate(Vec3::new(0.0, 0.0, -1.0)),
            Vec3::new(-FRAC_1_SQRT_2, 0.0, -FRAC_1_SQRT_2),
            1e-4,
        );
    }

    #[test]
    fn slerp_takes_shortest_arc_through_negation() {
        let a = Quat::IDENTITY;
        let b = -Quat::IDENTITY; // same rotation, opposite sign
        q_approx(Quat::slerp(a, b, 0.3), Quat::IDENTITY, 1e-5);
    }

    #[test]
    fn slerp_nlerp_fallback_for_tiny_angles() {
        let a = Quat::IDENTITY;
        let b = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 0.1);
        let mid = Quat::slerp(a, b, 0.5);
        v_approx(
            mid.rotate(Vec3::new(0.0, 0.0, -1.0)),
            Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 0.05)
                .rotate(Vec3::new(0.0, 0.0, -1.0)),
            1e-5,
        );
    }

    #[test]
    fn from_basis_identity() {
        let q = Quat::from_basis(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
        );
        q_approx(q, Quat::IDENTITY, 1e-6);
    }

    #[test]
    fn from_basis_yaw90_is_left() {
        // Looking left (forward = −X): right becomes −Z.
        let right = Vec3::new(0.0, 0.0, -1.0);
        let up = Vec3::new(0.0, 1.0, 0.0);
        let forward = Vec3::new(-1.0, 0.0, 0.0);
        let q = Quat::from_basis(right, up, forward);
        let expected = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0);
        v_approx(q.rotate(Vec3::new(0.0, 0.0, -1.0)), forward, 1e-6);
        q_approx(q, expected, 1e-5);
    }

    #[test]
    fn from_basis_round_trips_tilted_frame() {
        // Forward tilted down-forward; verify basis round-trip.
        let forward = Vec3::new(0.0, -1.0, -1.0).normalized();
        let right = forward.cross(Vec3::new(0.0, 1.0, 0.0)).normalized();
        let z_local = forward * -1.0;
        let up = z_local.cross(right);
        let q = Quat::from_basis(right, up, forward);
        v_approx(q.rotate(Vec3::new(0.0, 0.0, -1.0)), forward, 1e-5);
        v_approx(q.rotate(Vec3::new(1.0, 0.0, 0.0)), right, 1e-5);
        v_approx(q.rotate(Vec3::new(0.0, 1.0, 0.0)), up, 1e-5);
    }

    #[test]
    fn mat4_identity_transform() {
        let out = Mat4::IDENTITY.transform4([1.0, 2.0, 3.0, 1.0]);
        assert_eq!(out, [1.0, 2.0, 3.0, 1.0]);
    }

    #[test]
    fn mat4_mul_identity_is_noop() {
        let t = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(Mat4::IDENTITY.mul_mat(t), t);
        assert_eq!(t.mul_mat(Mat4::IDENTITY), t);
    }

    #[test]
    fn mat4_translation_transforms_points() {
        let t = Mat4::from_translation(Vec3::new(5.0, -2.0, 1.0));
        let out = t.transform4([1.0, 1.0, 1.0, 1.0]);
        assert_eq!(out, [6.0, -1.0, 2.0, 1.0]);
    }

    #[test]
    fn mat4_from_quat_matches_quat_rotate() {
        let q = Quat::from_euler_ypr_deg(35.0, -20.0, 10.0);
        let m = Mat4::from_quat(q);
        let v = Vec3::new(0.3, -1.2, 2.5);
        let mv = m.transform4([v.x, v.y, v.z, 1.0]);
        let qv = q.rotate(v);
        approx(mv[0], qv.x, 1e-5);
        approx(mv[1], qv.y, 1e-5);
        approx(mv[2], qv.z, 1e-5);
    }

    #[test]
    fn perspective_maps_near_and_far_planes_to_minus_plus_one() {
        let p = Mat4::perspective_rh(90.0f32.to_radians(), 1.0, 0.1, 100.0);
        let near = p.transform4([0.0, 0.0, -0.1, 1.0]);
        let far = p.transform4([0.0, 0.0, -100.0, 1.0]);
        approx(near[2] / near[3], -1.0, 1e-4);
        approx(far[2] / far[3], 1.0, 1e-4);
        // Point at unit right on the frustum edge at distance 1: ndc x = 1.
        let edge = p.transform4([1.0, 0.0, -1.0, 1.0]);
        approx(edge[0] / edge[3], 1.0, 1e-4);
        approx(edge[3], 1.0, 1e-6);
    }

    #[test]
    fn view_from_pose_translates_world_into_camera_space() {
        let pose = Vec3::new(0.0, 0.0, 5.0);
        let v = Mat4::view_from_pose(pose, Quat::IDENTITY);
        let out = v.transform4([0.0, 0.0, 0.0, 1.0]);
        assert_eq!(out, [0.0, 0.0, -5.0, 1.0]);
    }

    #[test]
    fn view_from_pose_rotates_for_yaw90() {
        // Camera yawed left 90° at origin: a point 2m to its left-front world
        // (−X) lands 2m ahead in camera space (−Z).
        let v = Mat4::view_from_pose(
            Vec3::ZERO,
            Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0),
        );
        let out = v.transform4([-2.0, 0.0, 0.0, 1.0]);
        approx(out[0], 0.0, 1e-6);
        approx(out[1], 0.0, 1e-6);
        approx(out[2], -2.0, 1e-5);
    }
}

//! Software rasterizer: world-space triangles to RGB8 color plus linear depth
//! (SPEC.md §12).
//!
//! The rasterizer is deliberately plain: one `f32` z-buffer, a bounding-box
//! scan with the top-left fill rule, flat per-batch shading, no filtering and no
//! anti-aliasing. Determinism (SPEC.md §1) comes from plain IEEE-754 `f32`
//! arithmetic, a fixed traversal order (batch, then triangle, then row, then
//! column), and the absence of threads, randomness, and system time. Identical
//! inputs therefore produce byte-identical `Frame` buffers on a given platform.

use lookwright_core::{CameraParams, CameraPose, Mat4, Vec3};

/// World-space triangles sharing one flat RGB color (SPEC.md §12).
///
/// The caller (perception crate) converts boxes into 12 triangles; the renderer
/// applies no lighting, materials, or vertex colors.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshBatch {
    pub triangles_m: Vec<[Vec3; 3]>,
    pub color: [u8; 3],
}

/// Rendered frame: RGB8 color buffer plus linear screen depth (SPEC.md §12).
///
/// `rgb` holds `width_px * height_px * 3` bytes, `depth01` holds
/// `width_px * height_px` samples in `[0, 1]`, both in row-major order.
/// Uncovered pixels are `(0, 0, 0)` with `depth01 == 1.0` (far plane).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub width_px: u32,
    pub height_px: u32,
    pub rgb: Vec<u8>,
    pub depth01: Vec<f32>,
}

impl Frame {
    /// Background frame: black RGB and `depth01 == 1.0` everywhere.
    ///
    /// Dimensions whose buffer size is not addressable yield empty buffers; the
    /// PNG codecs and [`crate::diff`] reject such frames instead of panicking.
    pub(crate) fn new(width_px: u32, height_px: u32) -> Self {
        match frame_sizes(width_px, height_px) {
            Some((pixels, channels)) => Self {
                width_px,
                height_px,
                rgb: vec![0; channels],
                depth01: vec![1.0; pixels],
            },
            None => Self {
                width_px,
                height_px,
                rgb: Vec::new(),
                depth01: Vec::new(),
            },
        }
    }

    /// Pixel count, or `None` when `width_px * height_px` is not addressable.
    #[cfg(test)]
    pub(crate) fn pixel_count(&self) -> Option<usize> {
        (self.width_px as usize).checked_mul(self.height_px as usize)
    }
}

/// `(pixels, rgb channels)` for a resolution, checked for overflow so callers
/// can reject impossible frames instead of allocating wrapped-around buffers.
pub(crate) fn frame_sizes(width_px: u32, height_px: u32) -> Option<(usize, usize)> {
    let pixels = (width_px as usize).checked_mul(height_px as usize)?;
    let channels = pixels.checked_mul(3)?;
    Some((pixels, channels))
}

/// Renders `batches` through `camera`/`params` at `res_px` (SPEC.md §12).
///
/// `res_px` is `[width_px, height_px]`. Batches are drawn in slice order; the
/// z-buffer resolves visibility between them.
///
/// v0.1 clips nothing except whole triangles behind the eye plane: geometry
/// beyond `far_m` interpolates `depth01 > 1.0`, so it loses to the `1.0`
/// background depth and stays black.
pub fn render(
    batches: &[MeshBatch],
    camera: &CameraPose,
    params: &CameraParams,
    res_px: [u32; 2],
) -> Frame {
    let width_px = res_px[0];
    let height_px = res_px[1];
    let mut frame = Frame::new(width_px, height_px);
    if width_px == 0 || height_px == 0 || frame.rgb.is_empty() {
        return frame;
    }

    let vp = params.perspective_matrix().mul_mat(camera.view_matrix());
    for batch in batches {
        for triangle_m in &batch.triangles_m {
            draw_triangle(&mut frame, &vp, triangle_m, batch.color);
        }
    }
    frame
}

/// A vertex in float pixel space (`x_px`/`y_px` at sub-pixel precision) plus its
/// screen-space depth in `[0, 1]`.
#[derive(Debug, Clone, Copy)]
struct ScreenVertex {
    x_px: f32,
    y_px: f32,
    depth01: f32,
}

impl ScreenVertex {
    fn is_finite(self) -> bool {
        self.x_px.is_finite() && self.y_px.is_finite() && self.depth01.is_finite()
    }
}

/// Clip space to float pixel space plus depth (SPEC.md §12).
///
/// `px = (ndc_x * 0.5 + 0.5) * width_px`, `py = (0.5 - ndc_y * 0.5) * height_px`
/// (y flips because screen space is y-down), `depth01 = ndc_z * 0.5 + 0.5`.
fn project(clip: [f32; 4], width_px: u32, height_px: u32) -> ScreenVertex {
    let inv_w = 1.0 / clip[3];
    let ndc_x = clip[0] * inv_w;
    let ndc_y = clip[1] * inv_w;
    let ndc_z = clip[2] * inv_w;
    ScreenVertex {
        x_px: (ndc_x * 0.5 + 0.5) * width_px as f32,
        y_px: (0.5 - ndc_y * 0.5) * height_px as f32,
        depth01: ndc_z * 0.5 + 0.5,
    }
}

/// Signed twice-area of the screen-space triangle `a, b, c`.
fn area2(a: ScreenVertex, b: ScreenVertex, c: ScreenVertex) -> f32 {
    edge_fn(a, b, c.x_px, c.y_px)
}

/// Edge function: signed area of `a, b, p`. Zero on the line through `a`/`b`,
/// positive on the interior side once the winding is normalized (`area2 > 0`).
fn edge_fn(a: ScreenVertex, b: ScreenVertex, p_x: f32, p_y: f32) -> f32 {
    (b.x_px - a.x_px) * (p_y - a.y_px) - (b.y_px - a.y_px) * (p_x - a.x_px)
}

/// Top-left fill rule (SPEC.md §12): a sample exactly on an edge is shaded by
/// exactly one of the two triangles sharing it, so shared edges never leave
/// seams and are never shaded twice.
///
/// Screen space is y-down and the winding is normalized to `area2 > 0`, so the
/// triangle interior sits on the `+x` side of a "left" edge, which is an edge
/// running toward `-y`; a "top" edge is horizontal and runs toward `+x`.
/// Reversing an edge flips both predicates, so exactly one of the two triangles
/// owning a shared edge passes the rule.
fn is_top_left(a: ScreenVertex, b: ScreenVertex) -> bool {
    let dx = b.x_px - a.x_px;
    let dy = b.y_px - a.y_px;
    (dy == 0.0 && dx > 0.0) || dy < 0.0
}
/// Screen-space barycentric depth. `ndc_z` (and therefore `depth01`) is affine
/// in screen coordinates because it already carries the `1/w` division, so
/// linear interpolation is exact - no perspective-correct divide is needed.
///
/// Interpolating relative to `depth01[0]` (using the identity
/// `edge[0] + edge[1] + edge[2] == area2`) keeps the result exactly equal to the
/// vertex depth when a triangle is flat in `z`, instead of a few ULPs off.
fn interpolate_depth01(edge: [f32; 3], area2: f32, depth01: [f32; 3]) -> f32 {
    let base = depth01[0];
    base + (edge[1] * (depth01[1] - base) + edge[2] * (depth01[2] - base)) / area2
}

/// Rasterizes one world-space triangle into `frame`.
fn draw_triangle(frame: &mut Frame, vp: &Mat4, triangle_m: &[Vec3; 3], color: [u8; 3]) {
    // v0.1 has no near-plane clipping (SPEC.md §12): if ANY vertex sits on or
    // behind the eye plane (`clip.w <= 0`) the whole triangle is dropped, so a
    // triangle straddling the eye plane disappears instead of being clipped.
    let mut clip = [[0.0f32; 4]; 3];
    for (slot, vertex_m) in clip.iter_mut().zip(triangle_m.iter()) {
        *slot = vp.transform4([vertex_m.x, vertex_m.y, vertex_m.z, 1.0]);
        if slot[3] <= 0.0 {
            return;
        }
    }

    let width_px = frame.width_px;
    let height_px = frame.height_px;
    let v0 = project(clip[0], width_px, height_px);
    let mut v1 = project(clip[1], width_px, height_px);
    let mut v2 = project(clip[2], width_px, height_px);
    if !(v0.is_finite() && v1.is_finite() && v2.is_finite()) {
        return;
    }

    let mut triangle_area2 = area2(v0, v1, v2);
    if triangle_area2 == 0.0 {
        return; // Zero-area triangle: no sample is strictly inside.
    }
    if triangle_area2 < 0.0 {
        // Normalize the winding so the top-left predicates below are meaningful.
        std::mem::swap(&mut v1, &mut v2);
        triangle_area2 = area2(v0, v1, v2);
    }

    // Edge `i` is opposite vertex `i` and spans `(v[i + 1], v[i + 2])`.
    let top_left = [
        is_top_left(v1, v2),
        is_top_left(v2, v0),
        is_top_left(v0, v1),
    ];

    // Bounding box of the triangle, clamped to the viewport.
    let x_lo = v0.x_px.min(v1.x_px).min(v2.x_px).floor().max(0.0) as i64;
    let x_hi = v0
        .x_px
        .max(v1.x_px)
        .max(v2.x_px)
        .ceil()
        .min(width_px as f32 - 1.0) as i64;
    let y_lo = v0.y_px.min(v1.y_px).min(v2.y_px).floor().max(0.0) as i64;
    let y_hi = v0
        .y_px
        .max(v1.y_px)
        .max(v2.y_px)
        .ceil()
        .min(height_px as f32 - 1.0) as i64;
    if x_hi < x_lo || y_hi < y_lo {
        return;
    }

    let depth01 = [v0.depth01, v1.depth01, v2.depth01];
    let row_stride = width_px as usize;
    for y_px in y_lo..=y_hi {
        let sample_y = y_px as f32 + 0.5;
        for x_px in x_lo..=x_hi {
            let sample_x = x_px as f32 + 0.5;
            let edge = [
                edge_fn(v1, v2, sample_x, sample_y),
                edge_fn(v2, v0, sample_x, sample_y),
                edge_fn(v0, v1, sample_x, sample_y),
            ];
            let inside = (edge[0] > 0.0 || (edge[0] == 0.0 && top_left[0]))
                && (edge[1] > 0.0 || (edge[1] == 0.0 && top_left[1]))
                && (edge[2] > 0.0 || (edge[2] == 0.0 && top_left[2]));
            if !inside {
                continue;
            }

            let pixel = (y_px as usize) * row_stride + x_px as usize;
            let sample_depth01 = interpolate_depth01(edge, triangle_area2, depth01);
            // Strict less-than: the top-left rule already disambiguates samples
            // shared by two triangles, so equal depths never double-shade.
            if sample_depth01 < frame.depth01[pixel] {
                frame.depth01[pixel] = sample_depth01;
                let base = pixel * 3;
                frame.rgb[base] = color[0];
                frame.rgb[base + 1] = color[1];
                frame.rgb[base + 2] = color[2];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lookwright_core::Quat;

    const RED: [u8; 3] = [255, 0, 0];
    const BLUE: [u8; 3] = [0, 0, 255];

    /// SPEC.md §13 camera fixture: fov 90°, aspect 1.0, near 0.1 m, far 100 m,
    /// camera at the origin looking down -Z.
    fn fixture() -> (CameraPose, CameraParams) {
        (
            CameraPose::new(Vec3::ZERO, Quat::IDENTITY),
            CameraParams::new(90.0, 0.1, 100.0, 1.0),
        )
    }

    fn screen(x_px: f32, y_px: f32) -> ScreenVertex {
        ScreenVertex {
            x_px,
            y_px,
            depth01: 0.5,
        }
    }

    /// Upright triangle spanning the pixel-space window `[25, 75]^2` at `z_m`.
    fn flat_triangle(z_m: f32) -> [Vec3; 3] {
        [
            Vec3::new(-0.5, -0.5, z_m),
            Vec3::new(0.5, -0.5, z_m),
            Vec3::new(0.0, 0.5, z_m),
        ]
    }

    fn batch(triangles_m: Vec<[Vec3; 3]>, color: [u8; 3]) -> MeshBatch {
        MeshBatch { triangles_m, color }
    }

    fn covered(frame: &Frame) -> usize {
        frame.depth01.iter().filter(|d| **d < 1.0).count()
    }

    fn pixel(frame: &Frame, x_px: u32, y_px: u32) -> [u8; 3] {
        let base = (y_px as usize * frame.width_px as usize + x_px as usize) * 3;
        [frame.rgb[base], frame.rgb[base + 1], frame.rgb[base + 2]]
    }

    #[test]
    fn background_frame_is_black_and_far() {
        let frame = Frame::new(3, 2);
        assert_eq!(frame.rgb.len(), 3 * 2 * 3);
        assert_eq!(frame.depth01.len(), 3 * 2);
        assert!(frame.rgb.iter().all(|b| *b == 0));
        assert!(frame.depth01.iter().all(|d| *d == 1.0));
        assert_eq!(frame.pixel_count(), Some(6));
    }

    #[test]
    fn empty_scene_stays_background() {
        let (camera, params) = fixture();
        let frame = render(&[], &camera, &params, [100, 100]);
        assert_eq!(frame.rgb.len(), 100 * 100 * 3);
        assert!(frame.rgb.iter().all(|b| *b == 0));
        assert!(frame.depth01.iter().all(|d| *d == 1.0));
    }

    #[test]
    fn zero_resolution_renders_empty_frame() {
        let (camera, params) = fixture();
        let frame = render(
            &[batch(vec![flat_triangle(-1.0)], RED)],
            &camera,
            &params,
            [0, 0],
        );
        assert_eq!(frame.width_px, 0);
        assert_eq!(frame.height_px, 0);
        assert!(frame.rgb.is_empty());
        assert!(frame.depth01.is_empty());
    }

    #[test]
    fn projection_maps_ndc_center_to_pixel_center_and_flips_y() {
        let center = project([0.0, 0.0, 0.0, 2.0], 100, 100);
        assert_eq!(center.x_px, 50.0);
        assert_eq!(center.y_px, 50.0);
        assert_eq!(center.depth01, 0.5);

        let up = project([0.0, 1.0, 0.0, 1.0], 100, 100);
        assert_eq!(up.y_px, 0.0);
        let left = project([-1.0, 0.0, 0.0, 1.0], 100, 100);
        assert_eq!(left.x_px, 0.0);
    }

    #[test]
    fn top_left_rule_classifies_screen_space_edges() {
        // y-down screen space with normalized winding: the interior sits on the
        // +x side of a left edge, so a left edge runs toward -y. A sample on a
        // top or left edge belongs to the triangle; shared edges are therefore
        // claimed by exactly one of the two triangles.
        assert!(is_top_left(screen(0.0, 0.0), screen(10.0, 0.0)), "top edge");
        assert!(
            is_top_left(screen(0.0, 10.0), screen(0.0, 0.0)),
            "left edge (runs toward -y)"
        );
        assert!(
            !is_top_left(screen(10.0, 0.0), screen(0.0, 0.0)),
            "reversed top edge"
        );
        assert!(
            !is_top_left(screen(0.0, 0.0), screen(0.0, 10.0)),
            "right edge (runs toward +y)"
        );
        assert!(
            !is_top_left(screen(0.0, 0.0), screen(10.0, 10.0)),
            "descending edge is neither top nor left"
        );
    }

    #[test]
    fn edge_function_sign_follows_winding() {
        let a = screen(0.0, 0.0);
        let b = screen(1.0, 0.0);
        let c = screen(0.0, 1.0);
        assert_eq!(area2(a, b, c), 1.0);
        assert_eq!(area2(a, c, b), -1.0);
        assert!(edge_fn(a, b, 0.5, 0.5) > 0.0);
        assert_eq!(edge_fn(a, b, 0.5, 0.0), 0.0);
        assert!(edge_fn(a, b, 0.5, -0.5) < 0.0);
    }

    #[test]
    fn flat_triangle_has_a_single_depth_value() {
        let (camera, params) = fixture();
        let frame = render(
            &[batch(vec![flat_triangle(-1.0)], RED)],
            &camera,
            &params,
            [100, 100],
        );
        assert!(covered(&frame) > 0);
        let expected = frame.depth01[50 * 100 + 50];
        assert!(expected < 1.0);
        for (index, depth) in frame.depth01.iter().enumerate() {
            if *depth < 1.0 {
                assert!(
                    (*depth - expected).abs() < 1e-6,
                    "pixel {index}: {depth} differs from {expected}"
                );
            }
        }
    }

    #[test]
    fn barycentric_depth_matches_vertex_depths() {
        let depths = [0.25, 0.5, 0.75];
        let area = 1.0;
        // Sample on vertex 0: only edge 0 (opposite v0) is non-zero.
        assert_eq!(interpolate_depth01([area, 0.0, 0.0], area, depths), 0.25);
        // Sample on the midpoint of the edge between v0 and v1.
        assert_eq!(
            interpolate_depth01([area * 0.5, area * 0.5, 0.0], area, depths),
            0.375
        );
        // Sample at the centroid.
        assert!(
            (interpolate_depth01([area / 3.0, area / 3.0, area / 3.0], area, depths) - 0.5).abs()
                < 1e-6
        );
    }

    #[test]
    fn zero_area_triangle_draws_nothing() {
        let (camera, params) = fixture();
        let degenerate = batch(
            vec![[
                Vec3::new(-0.5, -0.5, -1.0),
                Vec3::new(0.0, 0.0, -1.0),
                Vec3::new(0.5, 0.5, -1.0),
            ]],
            RED,
        );
        let frame = render(&[degenerate], &camera, &params, [100, 100]);
        assert_eq!(covered(&frame), 0);
    }

    #[test]
    fn vertex_at_or_behind_eye_plane_drops_the_triangle() {
        let (camera, params) = fixture();
        let straddling = batch(
            vec![[
                Vec3::new(-0.5, -0.5, -1.0),
                Vec3::new(0.5, -0.5, -1.0),
                Vec3::new(0.0, 0.5, 1.0),
            ]],
            RED,
        );
        let frame = render(&[straddling], &camera, &params, [100, 100]);
        assert_eq!(covered(&frame), 0, "no near-plane clipping in v0.1");
    }

    #[test]
    fn z_buffer_orders_batches_regardless_of_draw_order() {
        let (camera, params) = fixture();
        let far = batch(vec![flat_triangle(-1.0)], RED);
        let near = batch(vec![flat_triangle(-0.5)], BLUE);

        let far_first = render(&[far.clone(), near.clone()], &camera, &params, [100, 100]);
        let near_first = render(&[near, far], &camera, &params, [100, 100]);
        assert_eq!(pixel(&far_first, 50, 50), BLUE);
        assert_eq!(pixel(&near_first, 50, 50), BLUE);
        assert_eq!(far_first, near_first);
    }

    #[test]
    fn unaddressable_resolution_yields_empty_buffers() {
        assert!(frame_sizes(u32::MAX, u32::MAX).is_none());
        let frame = Frame::new(u32::MAX, u32::MAX);
        assert!(frame.rgb.is_empty());
        assert!(frame.depth01.is_empty());
    }
}

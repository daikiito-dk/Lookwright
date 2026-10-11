//! Integration tests for the `lookwright-renderer` public API (SPEC.md §12/§13).
//!
//! Every expectation is hand-computed from the SPEC camera fixture: fov_y 90°,
//! aspect 1.0, near 0.1 m, far 100 m, camera at the origin looking down −Z, and
//! a 100×100 frame. With `f = 1 / tan(45°) = 1` the mapping is
//! `px = (x / -z * 0.5 + 0.5) * 100` and `py = (0.5 - y / -z * 0.5) * 100`.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use lookwright_core::{CameraParams, CameraPose, Quat, Vec3};
use lookwright_renderer::{
    diff, read_rgb_png, render, write_depth_png, write_rgb_png, Frame, MeshBatch,
};

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];
const BLACK: [u8; 3] = [0, 0, 0];

/// SPEC.md §13 camera fixture.
fn camera() -> (CameraPose, CameraParams) {
    (
        CameraPose::new(Vec3::ZERO, Quat::IDENTITY),
        CameraParams::new(90.0, 0.1, 100.0, 1.0),
    )
}

fn batch(triangles_m: Vec<[Vec3; 3]>, color: [u8; 3]) -> MeshBatch {
    MeshBatch { triangles_m, color }
}

/// Upright triangle with its three corners at `z_m`.
fn triangle_at(z_m: f32) -> [Vec3; 3] {
    [
        Vec3::new(-0.5, -0.5, z_m),
        Vec3::new(0.5, -0.5, z_m),
        Vec3::new(0.0, 0.5, z_m),
    ]
}

/// A 1 m cube centred at `center_m`, as the 12 triangles the perception crate
/// emits for a box (SPEC.md §12).
fn box_batch(center_m: Vec3, size_m: f32, color: [u8; 3]) -> MeshBatch {
    let half = size_m * 0.5;
    let corner = |sx: f32, sy: f32, sz: f32| {
        Vec3::new(
            center_m.x + sx * half,
            center_m.y + sy * half,
            center_m.z + sz * half,
        )
    };
    let c = [
        corner(-1.0, -1.0, -1.0),
        corner(1.0, -1.0, -1.0),
        corner(1.0, 1.0, -1.0),
        corner(-1.0, 1.0, -1.0),
        corner(-1.0, -1.0, 1.0),
        corner(1.0, -1.0, 1.0),
        corner(1.0, 1.0, 1.0),
        corner(-1.0, 1.0, 1.0),
    ];
    // Six quads, counter-clockwise seen from outside.
    let quads = [
        [0, 1, 2, 3], // -z
        [5, 4, 7, 6], // +z (front)
        [4, 0, 3, 7], // -x
        [1, 5, 6, 2], // +x
        [4, 5, 1, 0], // -y
        [3, 2, 6, 7], // +y
    ];
    let mut triangles_m = Vec::with_capacity(12);
    for quad in quads {
        triangles_m.push([c[quad[0]], c[quad[1]], c[quad[2]]]);
        triangles_m.push([c[quad[0]], c[quad[2]], c[quad[3]]]);
    }
    batch(triangles_m, color)
}

fn pixel(frame: &Frame, x_px: u32, y_px: u32) -> [u8; 3] {
    let base = (y_px as usize * frame.width_px as usize + x_px as usize) * 3;
    [frame.rgb[base], frame.rgb[base + 1], frame.rgb[base + 2]]
}

fn depth01(frame: &Frame, x_px: u32, y_px: u32) -> f32 {
    frame.depth01[y_px as usize * frame.width_px as usize + x_px as usize]
}

fn covered_pixels(frame: &Frame) -> usize {
    frame.depth01.iter().filter(|d| **d < 1.0).count()
}

fn temp_png(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lookwright-renderer-it-{}-{name}.png",
        std::process::id()
    ))
}

/// Decodes a 16-bit grayscale PNG into big-endian samples.
fn decode_depth_png(path: &Path) -> (png::ColorType, png::BitDepth, u32, u32, Vec<u16>) {
    let decoder = png::Decoder::new(BufReader::new(File::open(path).expect("open depth png")));
    let mut reader = decoder.read_info().expect("read depth png info");
    let color_type = reader.info().color_type;
    let bit_depth = reader.info().bit_depth;
    let (width_px, height_px) = reader.info().size();
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let output = reader.next_frame(&mut buffer).expect("decode depth png");
    let values = buffer[..output.buffer_size()]
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    (color_type, bit_depth, width_px, height_px, values)
}

/// 1. One red triangle: color, background, and the exact depth value.
#[test]
fn render_single_triangle_color_background_and_depth() {
    let (pose, params) = camera();
    let frame = render(
        &[batch(vec![triangle_at(-1.0)], RED)],
        &pose,
        &params,
        [100, 100],
    );

    assert_eq!(frame.width_px, 100);
    assert_eq!(frame.height_px, 100);
    assert_eq!(frame.rgb.len(), 100 * 100 * 3);
    assert_eq!(frame.depth01.len(), 100 * 100);

    // Corners project to pixels (25, 75), (75, 75), (50, 25), so the pixel
    // covering (50.5, 50.5) is inside the triangle.
    assert_eq!(pixel(&frame, 50, 50), RED);
    assert_eq!(pixel(&frame, 50, 26), RED, "just inside the apex");
    assert_eq!(pixel(&frame, 25, 74), RED, "just inside the left edge");

    // Clearly outside the triangle.
    assert_eq!(pixel(&frame, 5, 5), BLACK);
    assert_eq!(pixel(&frame, 24, 74), BLACK, "just outside the left edge");
    assert_eq!(pixel(&frame, 95, 95), BLACK);

    // depth01 = window depth of the GL perspective matrix: with near 0.1 m and
    // far 100 m, z = -1 m gives ndc_z = (far + near - 2·far·near) / (far - near)
    // = 80.1 / 99.9, so depth01 = (80.1 / 99.9) * 0.5 + 0.5 = 0.9009009.
    // (It would only be 0.5 for a point at z = -2·near·far / (near + far).)
    let expected_depth01 = 0.900_900_9;
    let depth = depth01(&frame, 50, 50);
    assert!(
        (depth - expected_depth01).abs() < 1e-5,
        "depth01 {depth} != {expected_depth01}"
    );
    assert!(
        (depth - 0.5).abs() > 0.1,
        "sanity: ndc z is 0 only at z = -2·near·far / (near + far) = -0.1998 m, \
         so z = -1 m is not the 0.5 midpoint of the depth range"
    );

    // The triangle is flat in z, so every covered pixel shares its depth.
    for (index, value) in frame.depth01.iter().enumerate() {
        if *value < 1.0 {
            assert!(
                (value - depth).abs() < 1e-6,
                "pixel {index}: {value} differs from {depth}"
            );
        }
    }
}

/// 2. Z-buffering between batches.
#[test]
fn render_depth_orders_overlapping_batches() {
    let (pose, params) = camera();

    // Red at z = -1 m, blue at z = -0.5 m; the nearer blue triangle wins.
    let red_far = batch(vec![triangle_at(-1.0)], RED);
    let blue_near = batch(vec![triangle_at(-0.5)], BLUE);
    let frame = render(&[red_far, blue_near], &pose, &params, [100, 100]);
    assert_eq!(pixel(&frame, 50, 50), BLUE);

    // Reversed depths: red is now the nearer batch.
    let blue_far = batch(vec![triangle_at(-1.0)], BLUE);
    let red_near = batch(vec![triangle_at(-0.5)], RED);
    let frame = render(&[blue_far, red_near], &pose, &params, [100, 100]);
    assert_eq!(pixel(&frame, 50, 50), RED);
}

/// 3. Triangles at or behind the eye plane are culled entirely.
#[test]
fn render_culls_triangles_behind_the_camera() {
    let (pose, params) = camera();
    let behind = batch(vec![triangle_at(1.0)], RED);
    let frame = render(&[behind], &pose, &params, [100, 100]);

    assert_eq!(covered_pixels(&frame), 0);
    assert!(frame.rgb.iter().all(|byte| *byte == 0));
    assert!(frame.depth01.iter().all(|depth| *depth == 1.0));

    // One visible triangle next to one behind the camera: only the visible one
    // is drawn (v0.1 drops straddling triangles rather than clipping them).
    let visible = batch(vec![triangle_at(-1.0)], GREEN);
    let behind = batch(vec![triangle_at(1.0)], RED);
    let frame = render(&[visible, behind], &pose, &params, [100, 100]);
    assert_eq!(pixel(&frame, 50, 50), GREEN);
}

/// 4. Top-left fill rule: shared edges are shaded once and rendering is stable.
#[test]
fn render_top_left_rule_is_seam_free_and_stable() {
    let (pose, params) = camera();

    // Quad [25, 75]² split along its diagonal A–C; the pixel centres
    // (49.5, 50.5) and (50.5, 49.5) fall exactly on that shared edge.
    let a = Vec3::new(-0.5, -0.5, -1.0);
    let b = Vec3::new(0.5, -0.5, -1.0);
    let c = Vec3::new(0.5, 0.5, -1.0);
    let d = Vec3::new(-0.5, 0.5, -1.0);
    let triangle_a = [a, b, c];
    let triangle_b = [a, c, d];

    let both = batch(vec![triangle_a, triangle_b], WHITE);
    let only_a = batch(vec![triangle_a], WHITE);
    let only_b = batch(vec![triangle_b], WHITE);

    let frame_both = render(std::slice::from_ref(&both), &pose, &params, [100, 100]);
    let frame_a = render(&[only_a], &pose, &params, [100, 100]);
    let frame_b = render(&[only_b], &pose, &params, [100, 100]);

    // 50 columns × 50 rows of pixel centres lie inside the quad.
    assert_eq!(covered_pixels(&frame_both), 2500);
    assert_eq!(
        covered_pixels(&frame_a) + covered_pixels(&frame_b),
        2500,
        "every pixel is owned by exactly one triangle"
    );
    for y_px in 0..100u32 {
        for x_px in 0..100u32 {
            let inside = (25..75).contains(&x_px) && (25..75).contains(&y_px);
            assert_eq!(
                pixel(&frame_both, x_px, y_px),
                if inside { WHITE } else { BLACK },
                "pixel ({x_px}, {y_px})"
            );
        }
    }

    // The same batches rendered twice produce byte-identical frames.
    let again = render(&[both], &pose, &params, [100, 100]);
    assert_eq!(frame_both, again);
    assert_eq!(frame_both.rgb, again.rgb);
    assert_eq!(frame_both.depth01, again.depth01);
}

/// 5. Realistic box mesh: the box colour fills its silhouette.
#[test]
fn render_box_helper_fills_its_silhouette() {
    let (pose, params) = camera();
    let cube = box_batch(Vec3::new(0.0, 0.0, -5.0), 1.0, GREEN);
    assert_eq!(cube.triangles_m.len(), 12);

    let frame = render(&[cube], &pose, &params, [100, 100]);

    // The front face (±0.5 m at z = -4.5 m) projects to [44.444, 55.556]² in
    // pixel space and contains the whole silhouette, so exactly the 12×12 pixel
    // centres in that square are covered: indices 44..=55.
    assert_eq!(covered_pixels(&frame), 144);
    assert_eq!(pixel(&frame, 50, 50), GREEN);
    assert_eq!(pixel(&frame, 44, 44), GREEN);
    assert_eq!(pixel(&frame, 55, 55), GREEN);
    assert_eq!(pixel(&frame, 43, 50), BLACK, "left of the silhouette");
    assert_eq!(pixel(&frame, 56, 50), BLACK, "right of the silhouette");
    assert_eq!(pixel(&frame, 50, 43), BLACK, "above the silhouette");
    assert_eq!(pixel(&frame, 50, 56), BLACK, "below the silhouette");

    // Every covered pixel sits on the front face, so all depths are equal.
    let front = depth01(&frame, 50, 50);
    assert!(front < 1.0);
    for (index, value) in frame.depth01.iter().enumerate() {
        if *value < 1.0 {
            assert!(
                (value - front).abs() < 1e-6,
                "pixel {index}: {value} differs from {front}"
            );
        }
    }
}

/// 6. PNG round trips.
#[test]
fn png_round_trips_preserve_rgb_and_depth() {
    let (pose, params) = camera();
    let mut frame = render(
        &[
            batch(vec![triangle_at(-1.0)], RED),
            box_batch(Vec3::new(0.3, 0.0, -3.0), 0.5, GREEN),
        ],
        &pose,
        &params,
        [100, 100],
    );
    // Give the depth buffer a few non-flat values, including both extremes.
    frame.depth01[0] = 0.0;
    frame.depth01[1] = 1.0;
    frame.depth01[2] = 0.333_333;

    let rgb_path = temp_png("rgb");
    write_rgb_png(&rgb_path, &frame).expect("write rgb png");
    let decoded = read_rgb_png(&rgb_path).expect("read rgb png");
    assert_eq!(decoded.width_px, frame.width_px);
    assert_eq!(decoded.height_px, frame.height_px);
    assert_eq!(decoded.rgb, frame.rgb, "rgb bytes must round trip exactly");
    assert_eq!(decoded.depth01, vec![1.0; frame.depth01.len()]);

    // The file is 8-bit RGB (color type 2).
    let reader = png::Decoder::new(BufReader::new(File::open(&rgb_path).unwrap()))
        .read_info()
        .unwrap();
    assert_eq!(reader.info().color_type, png::ColorType::Rgb);
    assert_eq!(reader.info().bit_depth, png::BitDepth::Eight);
    assert_eq!(reader.output_buffer_size(), frame.rgb.len());

    let depth_path = temp_png("depth");
    write_depth_png(&depth_path, &frame).expect("write depth png");
    let (color_type, bit_depth, width_px, height_px, values) = decode_depth_png(&depth_path);
    assert_eq!(color_type, png::ColorType::Grayscale);
    assert_eq!(bit_depth, png::BitDepth::Sixteen);
    assert_eq!((width_px, height_px), (100, 100));
    assert_eq!(values.len(), frame.depth01.len());
    for (index, (value, depth)) in values.iter().zip(frame.depth01.iter()).enumerate() {
        let expected = (depth * 65535.0).round() as u16;
        assert!(
            value.abs_diff(expected) <= 1,
            "pixel {index}: got {value}, expected {expected}"
        );
    }
    assert_eq!(values[0], 0);
    assert_eq!(values[1], 65535);

    let _ = std::fs::remove_file(&rgb_path);
    let _ = std::fs::remove_file(&depth_path);
}

/// 7. Diff metrics.
#[test]
fn diff_reports_expected_metrics() {
    let (pose, params) = camera();
    let frame = render(
        &[box_batch(Vec3::new(0.0, 0.0, -5.0), 1.0, BLUE)],
        &pose,
        &params,
        [100, 100],
    );

    let identical = diff(&frame, &frame).expect("identical frames compare");
    assert_eq!(identical.mean_abs_diff, 0.0);
    assert_eq!(identical.max_abs_diff, 0);
    assert_eq!(identical.changed_pixel_ratio, 0.0);

    let mut changed = frame.clone();
    changed.rgb[0] += 10; // one channel of pixel (0, 0), +10
    let metrics = diff(&frame, &changed).expect("same size");
    let (width_px, height_px) = (frame.width_px as f32, frame.height_px as f32);
    let pixels = width_px * height_px;
    assert!(
        (metrics.mean_abs_diff - 10.0 / (pixels * 3.0)).abs() < 1e-6,
        "mean_abs_diff {}",
        metrics.mean_abs_diff
    );
    assert_eq!(metrics.max_abs_diff, 10);
    assert_eq!(metrics.changed_pixel_ratio, 1.0 / pixels);

    // A ±1 rounding difference must not count as a changed pixel (SPEC.md §12).
    let mut noisy = frame.clone();
    noisy.rgb[0] = 1;
    noisy.rgb[1] = 1;
    let metrics = diff(&frame, &noisy).expect("same size");
    assert_eq!(metrics.max_abs_diff, 1);
    assert_eq!(metrics.changed_pixel_ratio, 0.0);

    // Mismatched sizes are an error.
    let smaller = render(
        &[box_batch(Vec3::new(0.0, 0.0, -5.0), 1.0, BLUE)],
        &pose,
        &params,
        [50, 50],
    );
    let error = diff(&frame, &smaller).expect_err("size mismatch");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        error
            .to_string()
            .contains("image size mismatch 100x100 vs 50x50"),
        "{error}"
    );
}

/// 8. Determinism gate: the same scene renders byte-identically twice.
#[test]
fn render_is_byte_identical_across_runs() {
    let (pose, params) = camera();
    let scene = [
        box_batch(Vec3::new(0.0, 0.0, -5.0), 1.0, GREEN),
        box_batch(Vec3::new(-0.8, 0.2, -2.5), 0.6, RED),
        batch(vec![triangle_at(-20.0)], WHITE),
    ];

    let first = render(&scene, &pose, &params, [100, 100]);
    let second = render(&scene, &pose, &params, [100, 100]);

    assert_eq!(first.rgb, second.rgb);
    assert_eq!(first.depth01, second.depth01);
    assert_eq!(first, second);
    assert!(covered_pixels(&first) > 0, "scene must render something");
}

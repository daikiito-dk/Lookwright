//! Image comparison metrics for golden-image assertions (SPEC.md §9 A-05, §12).
//!
//! Metrics are computed in integer arithmetic and converted once to `f32`, so
//! identical inputs always produce bit-identical metrics (SPEC.md §1).

use std::io::{Error, ErrorKind, Result};

use crate::raster::{frame_sizes, Frame};

/// Comparison metrics between two RGB8 frames (SPEC.md §12).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffMetrics {
    /// Mean per-channel absolute difference on the 0..255 scale.
    pub mean_abs_diff: f32,
    /// Largest single-channel absolute difference.
    pub max_abs_diff: u8,
    /// Fraction of pixels whose max-channel absolute difference exceeds 1
    /// (SPEC.md §12: strictly greater than 1, so ±1 rounding noise is ignored).
    pub changed_pixel_ratio: f32,
}

/// Per-pixel max-channel difference above which a pixel counts as changed.
const CHANGED_PIXEL_DIFF_THRESHOLD: u16 = 1;

/// Compares two RGB8 frames of identical size (SPEC.md §12).
///
/// Returns [`ErrorKind::InvalidData`] when the dimensions or channel counts
/// differ.
pub fn diff(a: &Frame, b: &Frame) -> Result<DiffMetrics> {
    if a.width_px != b.width_px || a.height_px != b.height_px {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "image size mismatch {}x{} vs {}x{}",
                a.width_px, a.height_px, b.width_px, b.height_px
            ),
        ));
    }

    let Some((pixels, channels)) = frame_sizes(a.width_px, a.height_px) else {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "image size {}x{} is not addressable",
                a.width_px, a.height_px
            ),
        ));
    };
    if a.rgb.len() != channels || b.rgb.len() != channels {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "image channel count mismatch ({} vs {} rgb bytes) for {}x{}",
                a.rgb.len(),
                b.rgb.len(),
                a.width_px,
                a.height_px
            ),
        ));
    }
    if pixels == 0 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("image has zero pixels ({}x{})", a.width_px, a.height_px),
        ));
    }

    // Integer accumulation keeps the sums exact; the only rounding happens in
    // the final divisions.
    let mut sum_abs_diff: u64 = 0;
    let mut max_abs_diff: u16 = 0;
    let mut changed_pixels: usize = 0;
    for pixel in 0..pixels {
        let mut pixel_max_diff: u16 = 0;
        for channel in 0..3 {
            let index = pixel * 3 + channel;
            let diff = (a.rgb[index] as i32 - b.rgb[index] as i32).unsigned_abs() as u16;
            sum_abs_diff += u64::from(diff);
            max_abs_diff = max_abs_diff.max(diff);
            pixel_max_diff = pixel_max_diff.max(diff);
        }
        if pixel_max_diff > CHANGED_PIXEL_DIFF_THRESHOLD {
            changed_pixels += 1;
        }
    }

    Ok(DiffMetrics {
        mean_abs_diff: sum_abs_diff as f32 / channels as f32,
        // Every difference is at most 255 by construction.
        max_abs_diff: max_abs_diff as u8,
        changed_pixel_ratio: changed_pixels as f32 / pixels as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width_px: u32, height_px: u32, fill: u8) -> Frame {
        let mut frame = Frame::new(width_px, height_px);
        frame.rgb.iter_mut().for_each(|byte| *byte = fill);
        frame
    }

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-6, "approx failed: {a} vs {b}");
    }

    #[test]
    fn identical_frames_have_zero_metrics() {
        let a = frame(7, 5, 128);
        let b = frame(7, 5, 128);
        let metrics = diff(&a, &b).expect("same size");
        assert_eq!(
            metrics,
            DiffMetrics {
                mean_abs_diff: 0.0,
                max_abs_diff: 0,
                changed_pixel_ratio: 0.0,
            }
        );
    }

    #[test]
    fn one_channel_change_is_averaged_over_all_channels() {
        let a = frame(4, 4, 100);
        let mut b = a.clone();
        b.rgb[0] = 110;
        let metrics = diff(&a, &b).expect("same size");
        approx(metrics.mean_abs_diff, 10.0 / (4.0 * 4.0 * 3.0));
        assert_eq!(metrics.max_abs_diff, 10);
        approx(metrics.changed_pixel_ratio, 1.0 / 16.0);
    }

    #[test]
    fn negative_channel_differences_use_absolute_values() {
        let a = frame(2, 2, 200);
        let mut b = a.clone();
        b.rgb[5] = 190;
        let metrics = diff(&a, &b).expect("same size");
        approx(metrics.mean_abs_diff, 10.0 / 12.0);
        assert_eq!(metrics.max_abs_diff, 10);
        approx(metrics.changed_pixel_ratio, 0.25);
    }

    #[test]
    fn difference_of_one_does_not_count_as_changed() {
        let a = frame(2, 2, 50);
        let mut b = a.clone();
        b.rgb[0] = 51;
        b.rgb[11] = 49;
        let metrics = diff(&a, &b).expect("same size");
        assert_eq!(metrics.max_abs_diff, 1);
        assert_eq!(metrics.changed_pixel_ratio, 0.0);
        approx(metrics.mean_abs_diff, 2.0 / 12.0);
    }

    #[test]
    fn difference_of_two_counts_as_changed() {
        let a = frame(2, 2, 50);
        let mut b = a.clone();
        b.rgb[3] = 52;
        let metrics = diff(&a, &b).expect("same size");
        assert_eq!(metrics.max_abs_diff, 2);
        approx(metrics.changed_pixel_ratio, 0.25);
    }

    #[test]
    fn full_white_black_gap_reports_255() {
        let white = frame(3, 3, 255);
        let black = frame(3, 3, 0);
        let metrics = diff(&white, &black).expect("same size");
        assert_eq!(metrics.max_abs_diff, 255);
        approx(metrics.mean_abs_diff, 255.0);
        approx(metrics.changed_pixel_ratio, 1.0);
    }

    #[test]
    fn mismatched_dimensions_are_rejected() {
        let a = frame(4, 4, 0);
        let b = frame(4, 5, 0);
        let error = diff(&a, &b).expect_err("size mismatch");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("image size mismatch 4x4 vs 4x5"),
            "{error}"
        );
    }

    #[test]
    fn mismatched_channel_counts_are_rejected() {
        let a = frame(3, 3, 0);
        let mut b = a.clone();
        b.rgb.pop();
        let error = diff(&a, &b).expect_err("channel mismatch");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("channel count mismatch"),
            "{error}"
        );
    }

    #[test]
    fn zero_pixel_frames_are_rejected() {
        let a = Frame::new(0, 0);
        let b = Frame::new(0, 0);
        let error = diff(&a, &b).expect_err("zero pixels");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        assert!(error.to_string().contains("zero pixels"), "{error}");
    }
}

//! PNG codecs for rendered frames (SPEC.md §12).
//!
//! * RGB frames are written as 8-bit RGB (`ColorType::Rgb`, `BitDepth::Eight`).
//! * Depth frames are written as 16-bit grayscale with
//!   `value = (depth01 * 65535).round()`, saturating to `[0, 65535]`.
//! * [`read_rgb_png`] accepts 8-bit RGB only and returns depth `1.0` everywhere.
//!
//! Everything is single-threaded and never reads the clock, so identical inputs
//! produce identical files (SPEC.md §1).

use std::fs::File;
use std::io::{BufReader, BufWriter, Error, ErrorKind, Result};
use std::path::Path;

use png::{BitDepth, ColorType, Decoder, Encoder};

use crate::raster::{frame_sizes, Frame};

/// Validates a frame's buffers against its dimensions. Returns the pixel count.
fn check_frame(frame: &Frame) -> Result<usize> {
    if frame.width_px == 0 || frame.height_px == 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("frame has zero size {}x{}", frame.width_px, frame.height_px),
        ));
    }
    let Some((pixels, channels)) = frame_sizes(frame.width_px, frame.height_px) else {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "frame size {}x{} is not addressable",
                frame.width_px, frame.height_px
            ),
        ));
    };
    if frame.rgb.len() != channels {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "rgb buffer length {} does not match frame size {}x{} (expected {channels})",
                frame.rgb.len(),
                frame.width_px,
                frame.height_px
            ),
        ));
    }
    if frame.depth01.len() != pixels {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "depth buffer length {} does not match frame size {}x{} (expected {pixels})",
                frame.depth01.len(),
                frame.width_px,
                frame.height_px
            ),
        ));
    }
    Ok(pixels)
}

/// Writes the RGB buffer as an 8-bit RGB PNG (SPEC.md §12).
pub fn write_rgb_png(path: &Path, frame: &Frame) -> Result<()> {
    check_frame(frame)?;

    let file = File::create(path)?;
    let mut encoder = Encoder::new(BufWriter::new(file), frame.width_px, frame.height_px);
    encoder.set_color(ColorType::Rgb);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&frame.rgb)?;
    Ok(())
}

/// Writes `depth01` as a 16-bit grayscale PNG, `(depth01 * 65535).round()`
/// saturating to `[0, 65535]` (SPEC.md §12).
pub fn write_depth_png(path: &Path, frame: &Frame) -> Result<()> {
    check_frame(frame)?;

    // 16-bit PNG samples are big-endian; the encoder writes the bytes as given.
    let mut data = Vec::with_capacity(frame.depth01.len() * 2);
    for depth01 in &frame.depth01 {
        let value = (depth01 * 65535.0).round().clamp(0.0, 65535.0) as u16;
        data.extend_from_slice(&value.to_be_bytes());
    }

    let file = File::create(path)?;
    let mut encoder = Encoder::new(BufWriter::new(file), frame.width_px, frame.height_px);
    encoder.set_color(ColorType::Grayscale);
    encoder.set_depth(BitDepth::Sixteen);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    Ok(())
}

/// Reads an 8-bit RGB PNG (color type 2) into a [`Frame`].
///
/// Any other color type or bit depth is rejected with
/// [`ErrorKind::InvalidData`] and a message naming the actual format. The
/// returned frame has no depth information, so `depth01` is `1.0` everywhere.
pub fn read_rgb_png(path: &Path) -> Result<Frame> {
    let file = File::open(path)?;
    let mut reader = Decoder::new(BufReader::new(file)).read_info()?;

    let info = reader.info();
    let (color_type, bit_depth) = (info.color_type, info.bit_depth);
    if color_type != ColorType::Rgb || bit_depth != BitDepth::Eight {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "read_rgb_png requires 8-bit RGB (color type 2), found {}",
                describe_format(color_type, bit_depth)
            ),
        ));
    }

    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let output = reader.next_frame(&mut buffer)?;
    buffer.truncate(output.buffer_size());

    let Some((pixels, channels)) = frame_sizes(output.width, output.height) else {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "decoded image size {}x{} is not addressable",
                output.width, output.height
            ),
        ));
    };
    if buffer.len() != channels {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "decoded rgb buffer length {} does not match image size {}x{}",
                buffer.len(),
                output.width,
                output.height
            ),
        ));
    }

    Ok(Frame {
        width_px: output.width,
        height_px: output.height,
        rgb: buffer,
        depth01: vec![1.0; pixels],
    })
}

/// Human-readable PNG format, naming both the color type and its number as
/// encoded in the file header.
fn describe_format(color_type: ColorType, bit_depth: BitDepth) -> String {
    format!(
        "{color_type:?} {}-bit (color type {})",
        bit_depth as u8, color_type as u8
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_png(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lookwright-renderer-{}-{name}.png",
            std::process::id()
        ))
    }

    fn gradient_frame(width_px: u32, height_px: u32) -> Frame {
        let mut frame = Frame::new(width_px, height_px);
        for (index, byte) in frame.rgb.iter_mut().enumerate() {
            *byte = (index % 256) as u8;
        }
        let pixels = frame.depth01.len() as f32;
        for (index, depth01) in frame.depth01.iter_mut().enumerate() {
            *depth01 = index as f32 / pixels;
        }
        frame
    }

    /// Decodes any grayscale 16-bit PNG into big-endian samples.
    fn decode_depth_png(path: &Path) -> (png::ColorType, png::BitDepth, Vec<u16>) {
        let file = File::open(path).expect("open depth png");
        let mut reader = Decoder::new(BufReader::new(file))
            .read_info()
            .expect("read png info");
        let color_type = reader.info().color_type;
        let bit_depth = reader.info().bit_depth;
        let mut buffer = vec![0u8; reader.output_buffer_size()];
        let output = reader.next_frame(&mut buffer).expect("decode png");
        let values = buffer[..output.buffer_size()]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        (color_type, bit_depth, values)
    }

    /// Writes a PNG fixture with an arbitrary format, for rejection tests.
    fn write_fixture(path: &Path, color_type: ColorType, bit_depth: BitDepth, data: &[u8]) {
        let file = File::create(path).expect("create fixture");
        let mut encoder = Encoder::new(file, 2, 2);
        encoder.set_color(color_type);
        encoder.set_depth(bit_depth);
        let mut writer = encoder.write_header().expect("fixture header");
        writer.write_image_data(data).expect("fixture data");
    }

    #[test]
    fn rgb_round_trip_is_lossless() {
        let path = temp_png("rgb-round-trip");
        let frame = gradient_frame(5, 3);
        write_rgb_png(&path, &frame).expect("write rgb png");

        let decoded = read_rgb_png(&path).expect("read rgb png");
        assert_eq!(decoded.width_px, 5);
        assert_eq!(decoded.height_px, 3);
        assert_eq!(decoded.rgb, frame.rgb);
        assert_eq!(decoded.depth01, vec![1.0; 15]);

        // The file really is 8-bit RGB, as SPEC.md §12 requires.
        let reader = Decoder::new(BufReader::new(File::open(&path).unwrap()))
            .read_info()
            .unwrap();
        assert_eq!(reader.info().color_type, ColorType::Rgb);
        assert_eq!(reader.info().bit_depth, BitDepth::Eight);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn writes_are_byte_identical_across_runs() {
        let first = temp_png("determinism-a");
        let second = temp_png("determinism-b");
        let frame = gradient_frame(7, 5);

        write_rgb_png(&first, &frame).expect("write rgb png");
        write_rgb_png(&second, &frame).expect("write rgb png");
        let rgb_a = std::fs::read(&first).expect("read rgb a");
        assert_eq!(rgb_a, std::fs::read(&second).expect("read rgb b"));

        write_depth_png(&first, &frame).expect("write depth png");
        write_depth_png(&second, &frame).expect("write depth png");
        let depth_a = std::fs::read(&first).expect("read depth a");
        assert_eq!(depth_a, std::fs::read(&second).expect("read depth b"));

        let _ = std::fs::remove_file(&first);
        let _ = std::fs::remove_file(&second);
    }

    #[test]
    fn depth_png_round_trips_within_one() {
        let path = temp_png("depth-round-trip");
        let mut frame = Frame::new(4, 2);
        frame.depth01 = vec![0.0, 0.25, 0.5, 0.900_900_9, 1.0, 0.1, 0.333_333, 0.75];
        write_depth_png(&path, &frame).expect("write depth png");

        let (color_type, bit_depth, values) = decode_depth_png(&path);
        assert_eq!(color_type, ColorType::Grayscale);
        assert_eq!(bit_depth, BitDepth::Sixteen);
        assert_eq!(values.len(), frame.depth01.len());
        for (index, (value, depth01)) in values.iter().zip(frame.depth01.iter()).enumerate() {
            let expected = (depth01 * 65535.0).round() as u16;
            let difference = value.abs_diff(expected);
            assert!(
                difference <= 1,
                "pixel {index}: {value} vs {expected} (difference {difference})"
            );
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn depth_png_saturates_out_of_range_values() {
        let path = temp_png("depth-saturate");
        let mut frame = Frame::new(3, 1);
        frame.depth01 = vec![-0.25, 0.5, 1.5];
        write_depth_png(&path, &frame).expect("write depth png");

        let (_, _, values) = decode_depth_png(&path);
        assert_eq!(values, vec![0, 32768, 65535]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_rgb_png_rejects_grayscale() {
        let path = temp_png("grayscale");
        write_fixture(&path, ColorType::Grayscale, BitDepth::Eight, &[1, 2, 3, 4]);

        let error = read_rgb_png(&path).expect_err("grayscale is rejected");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        let message = error.to_string();
        assert!(message.contains("Grayscale"), "{message}");
        assert!(message.contains("8-bit"), "{message}");
        assert!(message.contains("color type 0"), "{message}");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_rgb_png_rejects_rgba() {
        let path = temp_png("rgba");
        write_fixture(
            &path,
            ColorType::Rgba,
            BitDepth::Eight,
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );

        let error = read_rgb_png(&path).expect_err("rgba is rejected");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        let message = error.to_string();
        assert!(message.contains("Rgba"), "{message}");
        assert!(message.contains("color type 6"), "{message}");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_rgb_png_rejects_sixteen_bit_rgb() {
        let path = temp_png("rgb16");
        let data = vec![0u8; 2 * 2 * 3 * 2];
        write_fixture(&path, ColorType::Rgb, BitDepth::Sixteen, &data);

        let error = read_rgb_png(&path).expect_err("16-bit RGB is rejected");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        let message = error.to_string();
        assert!(message.contains("Rgb"), "{message}");
        assert!(message.contains("16-bit"), "{message}");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let path = temp_png("does-not-exist");
        let _ = std::fs::remove_file(&path);
        let error = read_rgb_png(&path).expect_err("missing file is an error");
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn mismatched_buffers_are_rejected() {
        let path = temp_png("mismatched");
        let mut frame = gradient_frame(2, 2);
        frame.rgb.pop();
        let error = write_rgb_png(&path, &frame).expect_err("short rgb buffer");
        assert_eq!(error.kind(), ErrorKind::InvalidData);

        let mut frame = gradient_frame(2, 2);
        frame.depth01.pop();
        let error = write_depth_png(&path, &frame).expect_err("short depth buffer");
        assert_eq!(error.kind(), ErrorKind::InvalidData);

        let frame = Frame::new(0, 4);
        let error = write_rgb_png(&path, &frame).expect_err("zero width");
        assert_eq!(error.kind(), ErrorKind::InvalidInput);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn format_description_names_type_and_depth() {
        assert_eq!(
            describe_format(ColorType::Rgb, BitDepth::Eight),
            "Rgb 8-bit (color type 2)"
        );
        assert_eq!(
            describe_format(ColorType::Grayscale, BitDepth::Sixteen),
            "Grayscale 16-bit (color type 0)"
        );
    }
}

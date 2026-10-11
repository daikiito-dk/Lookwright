//! lookwright-renderer — deterministic CPU rasterizer, PNG codecs, and image
//! diff metrics. Implements SPEC.md §12 and the API contract in §13.
//!
//! # What it does
//!
//! [`render`] rasterizes world-space triangles ([`MeshBatch`]) through a camera
//! into a [`Frame`]: an RGB8 buffer plus a linear screen depth buffer
//! (`depth01` = NDC z mapped from `[-1, 1]` to `[0, 1]`). [`write_rgb_png`] and
//! [`write_depth_png`] persist frames, [`read_rgb_png`] reads RGB8 frames back,
//! and [`diff`] produces the golden-image comparison metrics of SPEC.md §9
//! (A-05).
//!
//! # Determinism (SPEC.md §1)
//!
//! Everything here is a single-threaded, `f32`-only, fixed-order computation:
//! no threads, no randomness, no system time, no `HashMap` iteration. The same
//! inputs therefore produce byte-identical frames and metrics on a given
//! platform, which is what makes golden-image CI comparisons meaningful.
//!
//! ```
//! use lookwright_core::{CameraParams, CameraPose, Quat, Vec3};
//! use lookwright_renderer::{render, MeshBatch};
//!
//! let triangle = MeshBatch {
//!     triangles_m: vec![[
//!         Vec3::new(-0.5, -0.5, -1.0),
//!         Vec3::new(0.5, -0.5, -1.0),
//!         Vec3::new(0.0, 0.5, -1.0),
//!     ]],
//!     color: [255, 0, 0],
//! };
//! let camera = CameraPose::new(Vec3::ZERO, Quat::IDENTITY);
//! let params = CameraParams::new(90.0, 0.1, 100.0, 1.0);
//!
//! let frame = render(&[triangle], &camera, &params, [100, 100]);
//! let base = (50 * 100 + 50) * 3;
//! assert_eq!(&frame.rgb[base..base + 3], &[255u8, 0, 0][..]);
//! ```

pub mod diff;
pub mod png_io;
pub mod raster;

pub use diff::{diff, DiffMetrics};
pub use png_io::{read_rgb_png, write_depth_png, write_rgb_png};
pub use raster::{render, Frame, MeshBatch};

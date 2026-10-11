//! lookwright-core — 6DoF trajectory engine, camera math, and spatial UI
//! anchors. Implements SPEC.md §1–§5 and the API contract in §13.

pub mod camera;
pub mod error;
pub mod math;
pub mod trajectory;
pub mod ui;

pub use camera::{CameraParams, CameraPose, Projector};
pub use error::Issue;
pub use math::{Mat4, Quat, Vec3};
pub use trajectory::{Event, Sample, Trajectory};
pub use ui::{Anchor, AnchorMode, UiPlacement, UiSpec, Wobble};

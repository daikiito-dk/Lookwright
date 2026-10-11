//! lookwright-perception — synthetic worlds, raycast, frustum culling, and
//! the contextual-AI perception feed. Implements SPEC.md §3 (worlds, presets,
//! glTF import), §3.2 raycast dispatch, and §8 (perception frames).
//!
//! Public API (SPEC.md §13):
//!
//! * [`World`] / [`WorldObject`] — parse, presets, glTF load, geometry
//! * [`cast_gaze`] / [`GazeHit`] — nearest-object gaze raycast
//! * [`Frustum`] — view-frustum AABB culling
//! * [`PerceptionFrame`] — the JSON / JSON-LD perception feed
//!
//! Determinism is a contract (SPEC.md §1): no HashMap iteration reaches any
//! output, no randomness, no system time, and library code never panics.

mod frame;
mod frustum;
mod gaze;
mod gltf_loader;
mod ray;
mod world;

pub use frame::PerceptionFrame;
pub use frustum::Frustum;
pub use gaze::{cast_gaze, GazeHit};
pub use world::{World, WorldObject};

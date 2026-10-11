//! `lookwright perception` (SPEC.md §10, §8) — the synthetic perception feed
//! used as mock input for contextual-AI agents.

use std::fs;
use std::path::{Path, PathBuf};

use lookwright_core::error::Issue;
use lookwright_core::{CameraParams, Projector, Trajectory};
use lookwright_perception::{PerceptionFrame, World};
use serde_json::Value;

/// Fixed feed defaults when not driven by a test config (documented in §10):
/// 70° fov, 16:10 frame, 30 hz sampling.
const FEED_FOV_Y_DEG: f32 = 70.0;
const FEED_RESOLUTION_PX: [u32; 2] = [640, 400];
const FEED_SAMPLE_HZ: f32 = 30.0;

pub fn run(
    world_ref: &str,
    trajectory_path: &Path,
    at_sec: Option<f32>,
    jsonld: bool,
    out: Option<&Path>,
) -> Result<i32, Issue> {
    let world = if World::preset_ids().contains(&world_ref) {
        World::from_preset(world_ref)?
    } else {
        World::load(Path::new(world_ref))?
    };
    let text = fs::read_to_string(trajectory_path).map_err(|e| {
        Issue::new(
            "P-01",
            trajectory_path.display().to_string(),
            format!("cannot read trajectory: {e}"),
        )
    })?;
    let traj = Trajectory::parse(&text)?;

    let aspect = FEED_RESOLUTION_PX[0] as f32 / FEED_RESOLUTION_PX[1] as f32;
    let params = CameraParams::new(FEED_FOV_Y_DEG, 0.05, 50.0, aspect);

    let capture = |t: f32| -> String {
        let sample = traj.sample(t);
        let projector = Projector::new(&sample.camera, &params, FEED_RESOLUTION_PX);
        let frame = PerceptionFrame::capture(t, &sample.camera, &world, &projector);
        if jsonld {
            frame.to_jsonld()
        } else {
            frame.to_json()
        }
    };

    let body = match at_sec {
        Some(t) if t < 0.0 => {
            return Err(Issue::new(
                "C-05",
                "--at-sec",
                format!("--at-sec must be >= 0, got {t}"),
            ));
        }
        Some(t) => capture(t),
        None => {
            let duration = traj.duration_sec();
            let dt = 1.0 / FEED_SAMPLE_HZ;
            // Tolerant floor, same rule as the runner (SPEC.md §6).
            let steps = (duration * FEED_SAMPLE_HZ + 1e-3).floor() as u32;
            let values: Vec<Value> = (0..=steps)
                .map(|i| {
                    let t = (i as f32 * dt).min(duration);
                    serde_json::from_str(&capture(t))
                })
                .collect::<Result<Vec<Value>, _>>()
                .map_err(|e| {
                    Issue::new(
                        "P-01",
                        "/",
                        format!("internal: perception frame serialization failed: {e}"),
                    )
                })?;
            serde_json::to_string_pretty(&values)
                .map_err(|e| Issue::new("P-01", "/", format!("cannot serialize frames: {e}")))?
        }
    };

    match out {
        Some(p) => {
            let mut body = body;
            body.push('\n');
            fs::write(p, body).map_err(|e| {
                Issue::new(
                    "P-01",
                    p.display().to_string(),
                    format!("cannot write output: {e}"),
                )
            })?;
        }
        None => println!("{body}"),
    }
    Ok(0)
}

/// Silence "unused" path import when at_sec branch early-returns.
#[allow(dead_code)]
fn _unused(_p: &PathBuf) {}

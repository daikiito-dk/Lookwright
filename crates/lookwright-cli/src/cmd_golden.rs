//! `lookwright golden update` (SPEC.md §10, §12).

use std::path::Path;

use lookwright_core::error::Issue;
use lookwright_renderer::write_rgb_png;

use crate::assert::AssertionSpec;
use crate::config::TestConfig;
use crate::runner;

/// Re-render every golden frame of a config (files are generated, never
/// hand-edited — AGENTS.md rule 7).
pub fn update(config_path: &Path) -> Result<i32, Issue> {
    let cfg = TestConfig::load(config_path)?;
    let world = cfg.load_world()?;
    let traj = cfg.load_trajectory()?;
    let params = cfg.camera_params();

    let mut written = 0u32;
    let mut any = false;
    for a in &cfg.assertions {
        let AssertionSpec::GoldenVisual { dir, times_sec, .. } = a else {
            continue;
        };
        any = true;
        std::fs::create_dir_all(dir).map_err(|e| {
            Issue::new(
                "P-01",
                dir.display().to_string(),
                format!("cannot create golden dir: {e}"),
            )
        })?;
        for &t in times_sec {
            let frame = runner::render_scene(&cfg, &world, &traj, &params, t);
            let path = runner::golden_path(dir, t);
            write_rgb_png(&path, &frame).map_err(|e| {
                Issue::new(
                    "P-01",
                    path.display().to_string(),
                    format!("cannot write golden frame: {e}"),
                )
            })?;
            println!("wrote {}", path.display());
            written += 1;
        }
    }
    if !any {
        eprintln!("no golden_visual assertions in config; nothing to update");
    } else {
        println!("updated {written} golden frame(s)");
    }
    Ok(0)
}

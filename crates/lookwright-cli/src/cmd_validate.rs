//! `lookwright validate` (SPEC.md §10) — coded validation without running.

use std::fs;
use std::path::Path;

use lookwright_core::error::Issue;
use lookwright_core::Trajectory;
use lookwright_perception::World;

use crate::config::TestConfig;

pub fn run(
    trajectory: Option<&Path>,
    world: Option<&str>,
    config: Option<&Path>,
) -> Result<i32, Issue> {
    if trajectory.is_none() && world.is_none() && config.is_none() {
        return Err(Issue::new(
            "P-01",
            "/",
            "nothing to validate: pass --trajectory, --world, or --config",
        ));
    }
    let mut bad = 0;

    if let Some(p) = trajectory {
        let result = fs::read_to_string(p)
            .map_err(|e| Issue::new("P-01", p.display().to_string(), format!("cannot read: {e}")))
            .and_then(|text| Trajectory::parse(&text));
        match result {
            Ok(t) => println!(
                "OK trajectory {} (duration {:.2}s, {} event(s))",
                p.display(),
                t.duration_sec(),
                t.events().len()
            ),
            Err(e) => {
                bad += 1;
                eprintln!("{e}");
            }
        }
    }

    if let Some(w) = world {
        let result = if World::preset_ids().contains(&w) {
            World::from_preset(w)
        } else {
            World::load(Path::new(w))
        };
        match result {
            Ok(world) => println!("OK world {w} ({} object(s))", world.objects().len()),
            Err(e) => {
                bad += 1;
                eprintln!("{e}");
            }
        }
    }

    if let Some(p) = config {
        match TestConfig::load(p) {
            Ok(cfg) => println!(
                "OK config {} ({} ui, {} assertion(s))",
                p.display(),
                cfg.ui.len(),
                cfg.assertions.len()
            ),
            Err(e) => {
                bad += 1;
                eprintln!("{e}");
            }
        }
    }

    Ok(i32::from(bad > 0) * 2)
}

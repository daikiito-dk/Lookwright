//! `lookwright worlds list|show` (SPEC.md §10, §3.1).

use lookwright_core::error::Issue;
use lookwright_perception::World;
use serde_json::json;

pub fn list() -> Result<i32, Issue> {
    for id in World::preset_ids() {
        let w = World::from_preset(id)?;
        println!("{id}\t{}", w.name());
    }
    Ok(0)
}

pub fn show(id: &str) -> Result<i32, Issue> {
    let w = World::from_preset(id)?;
    let objects: Vec<_> = w
        .objects()
        .iter()
        .map(|o| {
            let (min, max) = o.aabb();
            json!({
                "id": o.id(),
                "label": o.label(),
                "aabb_m": {
                    "min_m": [min.x, min.y, min.z],
                    "max_m": [max.x, max.y, max.z],
                },
                "color_rgb": o.color_rgb(),
            })
        })
        .collect();
    let doc = json!({ "id": id, "name": w.name(), "objects": objects });
    println!(
        "{}",
        serde_json::to_string_pretty(&doc).map_err(|e| Issue::new(
            "P-01",
            id,
            format!("cannot serialize world: {e}")
        ))?
    );
    Ok(0)
}

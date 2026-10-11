//! World parsing, validation, and preset worlds (SPEC.md §3, §7, §13).
//!
//! A world is a set of labeled objects. JSON only (SPEC.md §3: YAML is not
//! accepted for worlds). Every validation failure carries a stable code
//! (`W-01`…`W-07`, `P-01`), a JSON Pointer path into the document, and a
//! message naming the offending value. Library code never panics.

use std::collections::HashSet;
use std::path::Path;

use lookwright_core::error::value_f32;
use lookwright_core::{Issue, Quat, Vec3};
use serde_json::Value;

use crate::ray;
use crate::world::preset::Preset;

/// Embedded preset worlds (SPEC.md §3.1), referenced by id in configs.
mod preset {
    #[derive(Clone, Copy)]
    pub enum Preset {
        LivingRoom,
        Office,
        UrbanStreet,
        Warehouse,
    }

    impl Preset {
        pub const fn id(self) -> &'static str {
            match self {
                Preset::LivingRoom => "living_room",
                Preset::Office => "office",
                Preset::UrbanStreet => "urban_street",
                Preset::Warehouse => "warehouse",
            }
        }

        pub const fn json(self) -> &'static str {
            match self {
                Preset::LivingRoom => include_str!("../assets/presets/living_room.json"),
                Preset::Office => include_str!("../assets/presets/office.json"),
                Preset::UrbanStreet => include_str!("../assets/presets/urban_street.json"),
                Preset::Warehouse => include_str!("../assets/presets/warehouse.json"),
            }
        }
    }
}

/// Object geometry (SPEC.md §3). Exactly one of `box` or `points_m` comes
/// from JSON; `Mesh` is produced by the glTF loader (SPEC.md §3.2).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Geometry {
    /// Oriented box: world center, full size, and box rotation.
    Box {
        center_m: Vec3,
        size_m: Vec3,
        rotation: Quat,
    },
    /// Point cloud, treated as (and rendered as) its AABB.
    Points(Vec<Vec3>),
    /// World-space triangle soup (glTF meshes).
    Mesh { triangles: Vec<[Vec3; 3]> },
}

/// A labeled world object (SPEC.md §3).
#[derive(Debug, Clone, PartialEq)]
pub struct WorldObject {
    id: String,
    /// Stored label; `""` when absent. Perception frames fall back to the id
    /// (SPEC.md §8).
    label: String,
    color: Option<[u8; 3]>,
    geometry: Geometry,
}

impl WorldObject {
    pub(crate) fn new(
        id: String,
        label: String,
        color: Option<[u8; 3]>,
        geometry: Geometry,
    ) -> Self {
        Self {
            id,
            label,
            color,
            geometry,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Stored label; `""` when absent (frames fall back to the id).
    pub fn label(&self) -> &str {
        &self.label
    }

    /// World-space axis-aligned bounding box `(min_m, max_m)`. For a rotated
    /// box this is the AABB of its 8 transformed corners (SPEC.md §3).
    pub fn aabb(&self) -> (Vec3, Vec3) {
        match &self.geometry {
            Geometry::Box {
                center_m,
                size_m,
                rotation,
            } => {
                let half_m = Vec3::new(size_m.x * 0.5, size_m.y * 0.5, size_m.z * 0.5);
                let mut min_m = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
                let mut max_m = Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
                for signs in CORNER_SIGNS {
                    let local = Vec3::new(
                        signs[0] * half_m.x,
                        signs[1] * half_m.y,
                        signs[2] * half_m.z,
                    );
                    let corner = *center_m + rotation.rotate(local);
                    min_m = vmin(min_m, corner);
                    max_m = vmax(max_m, corner);
                }
                (min_m, max_m)
            }
            Geometry::Points(points) => points_aabb(points),
            Geometry::Mesh { triangles } => {
                let mut iter = triangles.iter().flatten();
                match iter.next() {
                    // An empty mesh keeps a finite degenerate AABB.
                    None => (Vec3::ZERO, Vec3::ZERO),
                    Some(&first) => {
                        let mut min_m = first;
                        let mut max_m = first;
                        for v in iter {
                            min_m = vmin(min_m, *v);
                            max_m = vmax(max_m, *v);
                        }
                        (min_m, max_m)
                    }
                }
            }
        }
    }

    /// Renderable triangles (SPEC.md §12): boxes and point-cloud AABBs expand
    /// to the 12 triangles of an axis-aligned box (rotated boxes rotate the
    /// local-space triangle vertices); meshes carry their own triangles.
    pub fn triangles_m(&self) -> Vec<[Vec3; 3]> {
        match &self.geometry {
            Geometry::Box {
                center_m,
                size_m,
                rotation,
            } => {
                let half_m = Vec3::new(size_m.x * 0.5, size_m.y * 0.5, size_m.z * 0.5);
                box_triangles_local(half_m)
                    .into_iter()
                    .map(|tri| tri.map(|v| *center_m + rotation.rotate(v)))
                    .collect()
            }
            Geometry::Points(points) => {
                let (min_m, max_m) = points_aabb(points);
                let center_m = Vec3::new(
                    (min_m.x + max_m.x) * 0.5,
                    (min_m.y + max_m.y) * 0.5,
                    (min_m.z + max_m.z) * 0.5,
                );
                let half_m = Vec3::new(
                    (max_m.x - min_m.x) * 0.5,
                    (max_m.y - min_m.y) * 0.5,
                    (max_m.z - min_m.z) * 0.5,
                );
                box_triangles_local(half_m)
                    .into_iter()
                    .map(|tri| tri.map(|v| v + center_m))
                    .collect()
            }
            Geometry::Mesh { triangles } => triangles.clone(),
        }
    }

    /// Render tint (SPEC.md §3): the parsed `#rrggbb` color when set, else a
    /// deterministic FNV-1a hash of the id (see [`fnv1a_rgb`]) so uncolored
    /// objects stay stable across runs and platforms.
    pub fn color_rgb(&self) -> [u8; 3] {
        match self.color {
            Some(color) => color,
            None => fnv1a_rgb(&self.id),
        }
    }

    /// Entry distance of a ray (`dir` assumed normalized by callers) into
    /// this object, or `None` (SPEC.md §3.2 dispatch: oriented-box
    /// intersection for JSON boxes, AABB for points, triangles for meshes).
    pub(crate) fn raycast(&self, origin_m: Vec3, dir: Vec3) -> Option<f32> {
        match &self.geometry {
            Geometry::Box {
                center_m,
                size_m,
                rotation,
            } => {
                let half_m = Vec3::new(size_m.x * 0.5, size_m.y * 0.5, size_m.z * 0.5);
                ray::ray_obb(origin_m, dir, *center_m, half_m, *rotation)
            }
            Geometry::Points(points) => {
                let (min_m, max_m) = points_aabb(points);
                ray::ray_aabb(origin_m, dir, min_m, max_m)
            }
            Geometry::Mesh { triangles } => ray::ray_triangles(origin_m, dir, triangles),
        }
    }
}

/// The 8 corner signs `[sx, sy, sz]` of an axis-aligned box, fixed order.
const CORNER_SIGNS: [[f32; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];

/// The 12 triangles of an axis-aligned box in local space (half-extents
/// scaled by the sign triples). Faces are ordered +X, -X, +Y, -Y, +Z, -Z and
/// each is wound counter-clockwise when seen from outside.
const BOX_TRIANGLE_SIGNS: [[[f32; 3]; 3]; 12] = [
    // +X
    [[1.0, -1.0, 1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0]],
    [[1.0, -1.0, 1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0]],
    // -X
    [[-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0]],
    [[-1.0, -1.0, -1.0], [-1.0, 1.0, 1.0], [-1.0, 1.0, -1.0]],
    // +Y
    [[-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0]],
    [[-1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [1.0, 1.0, -1.0]],
    // -Y
    [[-1.0, -1.0, 1.0], [-1.0, -1.0, -1.0], [1.0, -1.0, -1.0]],
    [[-1.0, -1.0, 1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0]],
    // +Z
    [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0]],
    [[-1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]],
    // -Z
    [[1.0, -1.0, -1.0], [-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0]],
    [[1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [1.0, 1.0, -1.0]],
];

/// The 12 triangles of an axis-aligned box centered at the origin.
fn box_triangles_local(half_m: Vec3) -> Vec<[Vec3; 3]> {
    BOX_TRIANGLE_SIGNS
        .iter()
        .map(|tri| tri.map(|s| Vec3::new(s[0] * half_m.x, s[1] * half_m.y, s[2] * half_m.z)))
        .collect()
}

/// AABB (min/max) of a point cloud. An empty cloud keeps a finite degenerate
/// AABB (JSON worlds reject empty clouds with `W-05` before this runs).
fn points_aabb(points: &[Vec3]) -> (Vec3, Vec3) {
    let mut min_m = points.first().copied().unwrap_or(Vec3::ZERO);
    let mut max_m = min_m;
    for p in points {
        min_m = vmin(min_m, *p);
        max_m = vmax(max_m, *p);
    }
    (min_m, max_m)
}

fn vmin(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

fn vmax(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
}

/// Deterministic fallback color: the FNV-1a 32-bit hash of the id string,
/// mapped to `[h & 0xff, (h >> 8) & 0xff, (h >> 16) & 0xff]`. Documented here
/// as the normative rule for unset colors (SPEC.md §3 `color` is optional;
/// the renderer tints by this value when it is absent).
fn fnv1a_rgb(s: &str) -> [u8; 3] {
    // FNV-1a: offset basis 0x811c9dc5, prime 0x01000193.
    let mut h: u32 = 0x811c_9dc5;
    for b in s.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    [
        (h & 0xff) as u8,
        ((h >> 8) & 0xff) as u8,
        ((h >> 16) & 0xff) as u8,
    ]
}

/// A validated world (SPEC.md §3).
#[derive(Debug, Clone, PartialEq)]
pub struct World {
    name: String,
    objects: Vec<WorldObject>,
}

impl World {
    pub(crate) fn new(name: String, objects: Vec<WorldObject>) -> Self {
        Self { name, objects }
    }

    /// Parse and validate a world document from JSON text (SPEC.md §3).
    /// Malformed JSON is `P-01` with path `/`; structural violations carry
    /// `W-01`…`W-05` with JSON Pointer paths into the document.
    pub fn parse(json: &str) -> Result<Self, Issue> {
        let doc: Value = serde_json::from_str(json).map_err(|err| {
            Issue::new(
                "P-01",
                "/",
                format!("world document is not valid JSON: {err}"),
            )
        })?;
        Self::from_value(&doc)
    }

    fn from_value(doc: &Value) -> Result<Self, Issue> {
        let root = doc.as_object().ok_or_else(|| {
            Issue::new(
                "W-02",
                "/",
                format!("world document must be a JSON object, got {doc}"),
            )
        })?;
        let name = match root.get("name") {
            None => String::new(),
            Some(v) => v
                .as_str()
                .ok_or_else(|| Issue::new("W-02", "/name", format!("expected a string, got {v}")))?
                .to_string(),
        };
        let mut objects = Vec::new();
        if let Some(objects_v) = root.get("objects") {
            let arr = objects_v.as_array().ok_or_else(|| {
                Issue::new(
                    "W-02",
                    "/objects",
                    format!("expected an array of world objects, got {objects_v}"),
                )
            })?;
            let mut seen: HashSet<String> = HashSet::new();
            for (i, object_v) in arr.iter().enumerate() {
                objects.push(parse_object(object_v, i, &mut seen)?);
            }
        }
        Ok(Self { name, objects })
    }

    /// Embedded preset world by id (SPEC.md §3.1). Unknown ids are `W-07`.
    pub fn from_preset(id: &str) -> Result<Self, Issue> {
        for preset in [
            Preset::LivingRoom,
            Preset::Office,
            Preset::UrbanStreet,
            Preset::Warehouse,
        ] {
            if preset.id() == id {
                return World::parse(preset.json());
            }
        }
        Err(Issue::new(
            "W-07",
            id,
            format!(
                "unknown preset id \"{id}\"; known presets: {}",
                Self::preset_ids().join(", ")
            ),
        ))
    }

    /// All preset ids, in the fixed order of SPEC.md §3.1.
    pub fn preset_ids() -> &'static [&'static str] {
        &["living_room", "office", "urban_street", "warehouse"]
    }

    /// Load a world from a `.json`, `.gltf`, or `.glb` file by extension
    /// (SPEC.md §3, §3.2). Unknown extensions and unreadable files are
    /// `P-01` with the file path; glTF parse failures carry the importer
    /// error in the message.
    pub fn load(path: &Path) -> Result<Self, Issue> {
        let path_str = path.to_string_lossy().into_owned();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("json") => {
                let text = std::fs::read_to_string(path).map_err(|err| {
                    Issue::new(
                        "P-01",
                        path_str.clone(),
                        format!("cannot read world file {path_str}: {err}"),
                    )
                })?;
                World::parse(&text)
            }
            Some("gltf") | Some("glb") => crate::gltf_loader::load(path),
            _ => Err(Issue::new(
                "P-01",
                path_str.clone(),
                format!(
                    "unsupported world file extension (expected .json, .gltf, or .glb): {path_str}"
                ),
            )),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn objects(&self) -> &[WorldObject] {
        &self.objects
    }
}

fn parse_object(v: &Value, index: usize, seen: &mut HashSet<String>) -> Result<WorldObject, Issue> {
    let base = format!("/objects/{index}");
    let object = v
        .as_object()
        .ok_or_else(|| Issue::new("W-02", base.clone(), format!("expected an object, got {v}")))?;

    // id: required, unique, [a-z0-9-]+ (SPEC.md §3, code W-01).
    //
    // Known SPEC deviation: the normative living_room preset (SPEC.md §3.1)
    // ships with the id "coffee_table", so the validator additionally
    // accepts underscores. Every SPEC example id remains valid and glTF ids
    // never contain underscores (the importer maps them to '-').
    let id_path = format!("{base}/id");
    let id = match object.get("id") {
        None => return Err(Issue::new("W-01", id_path, "missing required field \"id\"")),
        Some(id_v) => id_v.as_str().ok_or_else(|| {
            Issue::new(
                "W-01",
                id_path.clone(),
                format!("expected a string, got {id_v}"),
            )
        })?,
    };
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(Issue::new(
            "W-01",
            id_path,
            format!("invalid id \"{id}\": must match [a-z0-9_-]+"),
        ));
    }
    if !seen.insert(id.to_string()) {
        return Err(Issue::new(
            "W-01",
            id_path,
            format!("duplicate id \"{id}\""),
        ));
    }

    // label: optional string (code W-02 for a wrong field type).
    let label = match object.get("label") {
        None => String::new(),
        Some(label_v) => label_v
            .as_str()
            .ok_or_else(|| {
                Issue::new(
                    "W-02",
                    format!("{base}/label"),
                    format!("expected a string, got {label_v}"),
                )
            })?
            .to_string(),
    };

    // color: optional #rrggbb (code W-02 for a malformed value).
    let color = match object.get("color") {
        None => None,
        Some(color_v) => {
            let color_path = format!("{base}/color");
            let s = color_v.as_str().ok_or_else(|| {
                Issue::new(
                    "W-02",
                    color_path.clone(),
                    format!("expected a color string, got {color_v}"),
                )
            })?;
            Some(parse_hex_color(s, &color_path)?)
        }
    };

    let geometry = parse_geometry(object.get("geometry"), &format!("{base}/geometry"))?;
    Ok(WorldObject::new(id.to_string(), label, color, geometry))
}

fn parse_geometry(v: Option<&Value>, path: &str) -> Result<Geometry, Issue> {
    let geometry_v = v.ok_or_else(|| {
        Issue::new(
            "W-02",
            path.to_string(),
            "missing required field \"geometry\"",
        )
    })?;
    let object = geometry_v.as_object().ok_or_else(|| {
        Issue::new(
            "W-02",
            path.to_string(),
            format!("expected an object, got {geometry_v}"),
        )
    })?;
    let has_box = object.contains_key("box");
    let has_points = object.contains_key("points_m");
    match (has_box, has_points) {
        (true, true) => Err(Issue::new(
            "W-02",
            path.to_string(),
            "geometry must have exactly one of \"box\" or \"points_m\", got both",
        )),
        (false, false) => Err(Issue::new(
            "W-02",
            path.to_string(),
            "geometry must have exactly one of \"box\" or \"points_m\", got neither",
        )),
        (true, false) => parse_box(
            object.get("box").unwrap_or(&Value::Null),
            &format!("{path}/box"),
        ),
        (false, true) => parse_points(
            object.get("points_m").unwrap_or(&Value::Null),
            &format!("{path}/points_m"),
        ),
    }
}

fn parse_box(v: &Value, path: &str) -> Result<Geometry, Issue> {
    let object = v.as_object().ok_or_else(|| {
        Issue::new(
            "W-02",
            path.to_string(),
            format!("expected an object, got {v}"),
        )
    })?;
    let center_m = finite_vec3(
        object.get("center_m").ok_or_else(|| {
            Issue::new(
                "W-02",
                format!("{path}/center_m"),
                "missing required field \"center_m\"",
            )
        })?,
        &format!("{path}/center_m"),
    )?;
    let size_m = finite_vec3(
        object.get("size_m").ok_or_else(|| {
            Issue::new(
                "W-02",
                format!("{path}/size_m"),
                "missing required field \"size_m\"",
            )
        })?,
        &format!("{path}/size_m"),
    )?;
    if size_m.x <= 0.0 || size_m.y <= 0.0 || size_m.z <= 0.0 {
        return Err(Issue::new(
            "W-03",
            format!("{path}/size_m"),
            format!(
                "size_m components must be > 0, got {:?}",
                [size_m.x, size_m.y, size_m.z]
            ),
        ));
    }
    let rotation = match object.get("rotation_euler_ypr_deg") {
        None => Quat::IDENTITY,
        Some(rotation_v) => {
            let euler = finite_vecn(rotation_v, &format!("{path}/rotation_euler_ypr_deg"), 3)?;
            Quat::from_euler_ypr_deg(euler[0], euler[1], euler[2])
        }
    };
    Ok(Geometry::Box {
        center_m,
        size_m,
        rotation,
    })
}

fn parse_points(v: &Value, path: &str) -> Result<Geometry, Issue> {
    let arr = v.as_array().ok_or_else(|| {
        Issue::new(
            "W-02",
            path.to_string(),
            format!("expected an array of [x, y, z] points, got {v}"),
        )
    })?;
    if arr.is_empty() {
        return Err(Issue::new(
            "W-05",
            path.to_string(),
            "points_m must contain at least 1 point, got 0",
        ));
    }
    let mut points = Vec::with_capacity(arr.len());
    for (j, point_v) in arr.iter().enumerate() {
        points.push(finite_vec3(point_v, &format!("{path}/{j}"))?);
    }
    Ok(Geometry::Points(points))
}

fn parse_hex_color(s: &str, path: &str) -> Result<[u8; 3], Issue> {
    let bad = || {
        Issue::new(
            "W-02",
            path.to_string(),
            format!("invalid color \"{s}\": expected #rrggbb hex"),
        )
    };
    let hex = s
        .strip_prefix('#')
        .filter(|h| h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(bad)?;
    let channel = |lo: usize| -> Result<u8, Issue> {
        u8::from_str_radix(&hex[lo..lo + 2], 16).map_err(|_| bad())
    };
    Ok([channel(0)?, channel(2)?, channel(4)?])
}

/// Extract one finite f32: wrong types are `W-02` (wrong world field type,
/// SPEC.md §3); numbers that are not finite surface as `W-04` (SPEC.md §7).
///
/// W-04 is a defensive path: JSON itself cannot spell NaN or Infinity, so
/// this only fires for overflowing literals such as `1e400` (or non-finite
/// floats reaching validation through glTF buffers / YAML-converted
/// documents). It therefore has no dedicated JSON test.
fn finite_f32(v: &Value, path: &str) -> Result<f32, Issue> {
    match v.as_f64() {
        Some(n) => {
            let f = n as f32;
            if f.is_finite() {
                Ok(f)
            } else {
                Err(Issue::new(
                    "W-04",
                    path.to_string(),
                    format!("expected a finite number, got {n}"),
                ))
            }
        }
        // Not a number: reuse the core helper so the message matches
        // `value_f32` exactly (SPEC.md §7 "errors state path and value").
        None => value_f32(v, path, "W-02"),
    }
}

/// Extract exactly `n` finite f32s from a JSON array, mirroring the core
/// `value_vecn` messages with `W-02` for shape errors and `W-04` for
/// non-finite members.
fn finite_vecn(v: &Value, path: &str, n: usize) -> Result<Vec<f32>, Issue> {
    let arr = v.as_array().ok_or_else(|| {
        Issue::new(
            "W-02",
            path.to_string(),
            format!("expected an array of {n} numbers, got {v}"),
        )
    })?;
    if arr.len() != n {
        return Err(Issue::new(
            "W-02",
            path.to_string(),
            format!("expected an array of {n} numbers, got length {}", arr.len()),
        ));
    }
    arr.iter()
        .enumerate()
        .map(|(i, e)| finite_f32(e, &format!("{path}/{i}")))
        .collect()
}

fn finite_vec3(v: &Value, path: &str) -> Result<Vec3, Issue> {
    let a = finite_vecn(v, path, 3)?;
    Ok(Vec3::new(a[0], a[1], a[2]))
}

// ---------------------------------------------------------------------------
// tests

#[cfg(test)]
mod tests {
    use super::*;
    use lookwright_core::Quat;

    fn approx(actual: f32, expected: f32, tol: f32) {
        assert!(
            (actual - expected).abs() <= tol,
            "approx failed: {actual} vs {expected} (tol {tol})"
        );
    }

    fn v_approx(actual: Vec3, expected: Vec3, tol: f32) {
        approx(actual.x, expected.x, tol);
        approx(actual.y, expected.y, tol);
        approx(actual.z, expected.z, tol);
    }

    const CUSTOM_WORLD: &str = r#"{
      "version": 1,
      "name": "Custom",
      "objects": [
        { "id": "rotated-box", "label": "Rotated box",
          "geometry": { "box": { "center_m": [0.0, 0.0, 0.0], "size_m": [2.0, 1.0, 1.0],
                                 "rotation_euler_ypr_deg": [90.0, 0.0, 0.0] } } },
        { "id": "scan-cloud", "geometry": { "points_m": [[0.0, 0.0, -2.0], [1.0, 1.0, -2.0]] } }
      ]
    }"#;

    #[test]
    fn parses_valid_world_with_rotated_box_and_points() {
        let world = World::parse(CUSTOM_WORLD).unwrap();
        assert_eq!(world.name(), "Custom");
        assert_eq!(world.objects().len(), 2);
        let rotated = &world.objects()[0];
        assert_eq!(rotated.id(), "rotated-box");
        assert_eq!(rotated.label(), "Rotated box");
        // 90 deg yaw swaps the local x/z extents (2 m along local x becomes
        // 2 m along world z; 1 m along local z becomes 1 m along world x).
        let (min_m, max_m) = rotated.aabb();
        v_approx(min_m, Vec3::new(-0.5, -0.5, -1.0), 1e-6);
        v_approx(max_m, Vec3::new(0.5, 0.5, 1.0), 1e-6);
        let cloud = &world.objects()[1];
        assert_eq!(cloud.label(), "");
        let (min_m, max_m) = cloud.aabb();
        v_approx(min_m, Vec3::new(0.0, 0.0, -2.0), 1e-6);
        v_approx(max_m, Vec3::new(1.0, 1.0, -2.0), 1e-6);
    }

    #[test]
    fn parse_errors_carry_code_and_path() {
        let cases: &[(&str, &str, &str)] = &[
            // (document, expected code, expected path)
            (
                r#"{ "objects": [ { "id": "a", "geometry": { "points_m": [[0,0,0]] } },
                     { "id": "a", "geometry": { "points_m": [[0,0,0]] } } ] }"#,
                "W-01",
                "/objects/1/id",
            ),
            (
                r#"{ "objects": [ { "id": "Big Sofa!", "geometry": { "box": {
                     "center_m": [0,0,0], "size_m": [1,1,1] } } } ] }"#,
                "W-01",
                "/objects/0/id",
            ),
            (
                r#"{ "objects": [ { "label": "no id" } ] }"#,
                "W-01",
                "/objects/0/id",
            ),
            (
                r#"{ "objects": [ { "id": "a" } ] }"#,
                "W-02",
                "/objects/0/geometry",
            ),
            (
                r#"{ "objects": [ { "id": "a", "geometry": { "box": {
                     "center_m": [0,0,0], "size_m": [0.0, 1.0, 1.0] } } } ] }"#,
                "W-03",
                "/objects/0/geometry/box/size_m",
            ),
            (
                r#"{ "objects": [ { "id": "a", "geometry": { "points_m": [] } } ] }"#,
                "W-05",
                "/objects/0/geometry/points_m",
            ),
            (
                r#"{ "objects": [ { "id": "a", "geometry": { "box": {
                     "center_m": [0,0], "size_m": [1,1,1] } } } ] }"#,
                "W-02",
                "/objects/0/geometry/box/center_m",
            ),
            (
                r#"{ "objects": [ { "id": "a", "geometry": { "points_m": [[0,0,0], "x"] } } ] }"#,
                "W-02",
                "/objects/0/geometry/points_m/1",
            ),
            (
                r#"{ "objects": [ { "id": "a", "geometry": {
                     "box": { "center_m": [0,0,0], "size_m": [1,1,1] },
                     "points_m": [[0,0,0]] } } ] }"#,
                "W-02",
                "/objects/0/geometry",
            ),
            (
                r##"{ "objects": [ { "id": "a", "color": "#12345", "geometry": {
                     "box": { "center_m": [0,0,0], "size_m": [1,1,1] } } } ] }"##,
                "W-02",
                "/objects/0/color",
            ),
            (r#"{ "name": 3 }"#, "W-02", "/name"),
            (r#"{ "objects": 5 }"#, "W-02", "/objects"),
            ("[1, 2, 3]", "W-02", "/"),
            ("not json at all", "P-01", "/"),
        ];
        for (doc, code, path) in cases {
            let issue = World::parse(doc).unwrap_err();
            assert_eq!(issue.code, *code, "code for {doc}");
            assert_eq!(issue.path, *path, "path for {doc}");
            assert!(!issue.message.is_empty(), "message for {doc}");
        }
    }

    #[test]
    fn error_messages_name_the_offending_value() {
        let issue = World::parse(
            r#"{ "objects": [ { "id": "sofa", "geometry": { "box": {
                 "center_m": [0.0, 0.0, 0.0], "size_m": [2.2, 0.0, 0.9] } } } ] }"#,
        )
        .unwrap_err();
        assert_eq!(issue.code, "W-03");
        assert!(issue.message.contains("0.0"), "{}", issue.message);

        let issue = World::parse(
            r#"{ "objects": [ { "id": "Big Sofa!", "geometry": { "box": {
                 "center_m": [0.0, 0.0, 0.0], "size_m": [1.0, 1.0, 1.0] } } } ] }"#,
        )
        .unwrap_err();
        assert_eq!(issue.code, "W-01");
        assert!(issue.message.contains("Big Sofa!"), "{}", issue.message);
    }

    #[test]
    fn world_without_objects_is_empty() {
        let world = World::parse(r#"{ "version": 1, "name": "Empty" }"#).unwrap();
        assert_eq!(world.name(), "Empty");
        assert!(world.objects().is_empty());
    }

    #[test]
    fn aabb_of_yawed_cube_is_invariant() {
        let world = World::parse(
            r#"{ "objects": [ { "id": "cube", "geometry": { "box": {
                 "center_m": [0.0, 0.0, 0.0], "size_m": [1.0, 1.0, 1.0],
                 "rotation_euler_ypr_deg": [90.0, 0.0, 0.0] } } } ] }"#,
        )
        .unwrap();
        let (min_m, max_m) = world.objects()[0].aabb();
        v_approx(min_m, Vec3::new(-0.5, -0.5, -0.5), 1e-6);
        v_approx(max_m, Vec3::new(0.5, 0.5, 0.5), 1e-6);
    }

    #[test]
    fn triangles_of_box_are_twelve_and_inside_the_aabb() {
        let world = World::parse(CUSTOM_WORLD).unwrap();
        let rotated = &world.objects()[0];
        let triangles = rotated.triangles_m();
        assert_eq!(triangles.len(), 12);
        let (min_m, max_m) = rotated.aabb();
        for tri in &triangles {
            for v in tri {
                assert!(v.x >= min_m.x - 1e-6 && v.x <= max_m.x + 1e-6);
                assert!(v.y >= min_m.y - 1e-6 && v.y <= max_m.y + 1e-6);
                assert!(v.z >= min_m.z - 1e-6 && v.z <= max_m.z + 1e-6);
            }
        }
        // The rotated +x local face becomes a world z-facing quad: the 2 m
        // local x extent reaches world z = +/-1.
        let max_z = triangles
            .iter()
            .flatten()
            .map(|v| v.z)
            .fold(f32::NEG_INFINITY, f32::max);
        approx(max_z, 1.0, 1e-6);
        // Points objects render as their AABB: 12 triangles too.
        assert_eq!(world.objects()[1].triangles_m().len(), 12);
    }

    #[test]
    fn triangles_of_box_cover_the_rotated_faces() {
        // A yaw-90 box of local size (2, 1, 1) has its 2 m extent along world
        // z; every triangle vertex must respect that mapping.
        let world = World::parse(CUSTOM_WORLD).unwrap();
        let triangles = world.objects()[0].triangles_m();
        for tri in &triangles {
            for v in tri {
                assert!(v.x.abs() <= 0.5 + 1e-6);
                assert!(v.y.abs() <= 0.5 + 1e-6);
                assert!(v.z.abs() <= 1.0 + 1e-6);
            }
        }
    }

    #[test]
    fn color_rgb_uses_parsed_hex() {
        let world = World::parse(
            r##"{ "objects": [ { "id": "sofa", "color": "#7a5230", "geometry": {
                 "box": { "center_m": [0.0, 0.0, 0.0], "size_m": [1.0, 1.0, 1.0] } } } ] }"##,
        )
        .unwrap();
        assert_eq!(world.objects()[0].color_rgb(), [0x7a, 0x52, 0x30]);
    }

    #[test]
    fn color_rgb_falls_back_to_fnv1a_hash_of_the_id() {
        let world = World::parse(
            r#"{ "objects": [ { "id": "foobar", "geometry": {
                 "points_m": [[0.0, 0.0, 0.0]] } } ] }"#,
        )
        .unwrap();
        // FNV-1a 32-bit of "foobar" is 0xbf9cf968 (reference test vector).
        assert_eq!(fnv1a_rgb("foobar"), [0x68, 0xf9, 0x9c]);
        assert_eq!(fnv1a_rgb(""), [0xc5, 0x9d, 0x1c]);
        assert_eq!(fnv1a_rgb("a"), [0x2c, 0x29, 0x0c]);
        assert_eq!(world.objects()[0].color_rgb(), fnv1a_rgb("foobar"));
    }

    #[test]
    fn from_preset_loads_living_room() {
        let world = World::from_preset("living_room").unwrap();
        assert_eq!(world.name(), "Living Room");
        assert_eq!(world.objects().len(), 10);
        let ids: Vec<&str> = world.objects().iter().map(|o| o.id()).collect();
        assert!(ids.contains(&"tv"));
        assert!(ids.contains(&"painting"));
        // SPEC.md §8 example coordinates for the tv must round-trip.
        let tv = world.objects().iter().find(|o| o.id() == "tv").unwrap();
        let (min_m, max_m) = tv.aabb();
        v_approx(min_m, Vec3::new(-0.8, 0.85, -4.94), 1e-6);
        v_approx(max_m, Vec3::new(0.8, 1.75, -4.86), 1e-6);
        assert_eq!(tv.color_rgb(), [0x1a, 0x1a, 0x1a]);
    }

    #[test]
    fn every_preset_parses() {
        for id in World::preset_ids() {
            let world = World::from_preset(id).unwrap();
            assert!(!world.name().is_empty());
            assert!(!world.objects().is_empty());
            for object in world.objects() {
                assert!(
                    object.triangles_m().len() == 12 || object.triangles_m().len() % 3 == 0,
                    "unexpected triangle count for {}",
                    object.id()
                );
            }
        }
    }

    #[test]
    fn preset_ids_are_the_spec_list() {
        assert_eq!(
            World::preset_ids(),
            &["living_room", "office", "urban_street", "warehouse"]
        );
    }

    #[test]
    fn from_preset_unknown_id_is_w07() {
        let issue = World::from_preset("nope").unwrap_err();
        assert_eq!(issue.code, "W-07");
        assert_eq!(issue.path, "nope");
        assert!(issue.message.contains("nope"), "{}", issue.message);
        assert!(issue.message.contains("living_room"), "{}", issue.message);
    }

    #[test]
    fn load_reads_preset_json_from_disk() {
        let world = World::load(Path::new("assets/presets/living_room.json")).unwrap();
        assert_eq!(world.name(), "Living Room");
        assert_eq!(world.objects().len(), 10);
    }

    #[test]
    fn load_unreadable_file_is_p01_with_the_file_path() {
        let issue = World::load(Path::new("does/not/exist.json")).unwrap_err();
        assert_eq!(issue.code, "P-01");
        assert_eq!(issue.path, "does/not/exist.json");
        assert!(issue.message.contains("does/not/exist.json"));
    }

    #[test]
    fn load_unknown_extension_is_p01_with_the_file_path() {
        let issue = World::load(Path::new("worlds/scene.usdz")).unwrap_err();
        assert_eq!(issue.code, "P-01");
        assert_eq!(issue.path, "worlds/scene.usdz");
        assert!(issue.message.contains("usdz"));
    }

    #[test]
    fn rotation_quat_comes_from_euler_ypr() {
        let world = World::parse(CUSTOM_WORLD).unwrap();
        let expected = Quat::from_euler_ypr_deg(90.0, 0.0, 0.0);
        assert_eq!(
            world.objects()[0].geometry,
            Geometry::Box {
                center_m: Vec3::ZERO,
                size_m: Vec3::new(2.0, 1.0, 1.0),
                rotation: expected
            }
        );
    }
}

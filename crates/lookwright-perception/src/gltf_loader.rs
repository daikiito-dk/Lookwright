//! glTF 2.0 world import (SPEC.md §3.2).
//!
//! `World::load` dispatches `.gltf`/`.glb` files here. Every scene node with
//! a mesh becomes one [`WorldObject`] carrying the world-space triangles of
//! all its primitives (node transforms applied as T·R·S); the base color
//! factor of the first primitive's material is used as the render tint.
//! Parse and IO failures surface as `P-01` with the file path and the
//! importer error in the message (SPEC.md §7).

use std::collections::HashSet;
use std::path::Path;

use lookwright_core::{Issue, Mat4, Quat, Vec3};

use crate::world::{Geometry, World, WorldObject};

/// Import a glTF file as a world. Deterministic by construction: scenes,
/// nodes, primitives, and triangles are visited in document order; ids are
/// deduplicated with `-2`, `-3` suffixes.
pub(crate) fn load(path: &Path) -> Result<World, Issue> {
    let path_str = path.to_string_lossy().into_owned();
    let (doc, buffers, _images) = gltf::import(path).map_err(|err| {
        Issue::new(
            "P-01",
            path_str.clone(),
            format!("cannot import glTF file {path_str}: {err}"),
        )
    })?;

    // Walk the default scene, or every scene in document order when the file
    // does not declare one (SPEC.md §3.2).
    let name = doc
        .default_scene()
        .and_then(|scene| scene.name())
        .unwrap_or("")
        .to_string();
    let scenes: Vec<gltf::Scene> = match doc.default_scene() {
        Some(scene) => vec![scene],
        None => doc.scenes().collect(),
    };

    let mut objects: Vec<WorldObject> = Vec::new();
    let mut used: HashSet<String> = HashSet::new();
    // Explicit stack (roots and children pushed in reverse) preserves
    // depth-first document order without unbounded recursion.
    let mut stack: Vec<(gltf::Node, Mat4)> = Vec::new();
    for scene in scenes {
        let roots: Vec<gltf::Node> = scene.nodes().collect();
        for node in roots.into_iter().rev() {
            stack.push((node, Mat4::IDENTITY));
        }
        while let Some((node, parent)) = stack.pop() {
            let world = parent.mul_mat(node_matrix(node.transform()));
            if let Some(mesh) = node.mesh() {
                objects.push(build_object(&node, &mesh, &world, &buffers, &mut used));
            }
            let children: Vec<gltf::Node> = node.children().collect();
            for child in children.into_iter().rev() {
                stack.push((child, world));
            }
        }
    }

    // Defensive finite guard on all imported numbers (SPEC.md §3 W-04):
    // glTF binary buffers can carry NaN/inf vertex coordinates that JSON
    // worlds can never express.
    for object in &objects {
        if object
            .triangles_m()
            .iter()
            .flatten()
            .any(|v| !v.is_finite())
        {
            return Err(Issue::new(
                "W-04",
                path_str.clone(),
                format!(
                    "mesh \"{}\" in {path_str} contains non-finite vertex coordinates",
                    object.id()
                ),
            ));
        }
    }

    Ok(World::new(name, objects))
}

/// Local node matrix from the decomposed TRS, composed as T·R·S per node
/// (SPEC.md §3.2). glTF scales may be non-uniform, hence the full affine
/// product rather than a rigid transform.
fn node_matrix(t: gltf::scene::Transform) -> Mat4 {
    let (translation, rotation, scale) = t.decomposed();
    // glTF quaternions are [x, y, z, w] and unit by spec; normalize
    // defensively (a zero quaternion stays identity via `Quat::normalized`).
    let rotation = Quat::new(rotation[0], rotation[1], rotation[2], rotation[3]).normalized();
    Mat4::from_translation(Vec3::new(translation[0], translation[1], translation[2]))
        .mul_mat(Mat4::from_quat(rotation))
        .mul_mat(scale_matrix(scale))
}

fn scale_matrix(s: [f32; 3]) -> Mat4 {
    Mat4::from_cols(
        [s[0], 0.0, 0.0, 0.0],
        [0.0, s[1], 0.0, 0.0],
        [0.0, 0.0, s[2], 0.0],
        [0.0, 0.0, 0.0, 1.0],
    )
}

fn build_object(
    node: &gltf::Node,
    mesh: &gltf::Mesh,
    world: &Mat4,
    buffers: &[gltf::buffer::Data],
    used: &mut HashSet<String>,
) -> WorldObject {
    // id priority: node name, then mesh name, then node-<index>, sanitized
    // to [a-z0-9-]+ with -2/-3 suffixes on collisions (SPEC.md §3.2).
    let raw_name = node.name().or_else(|| mesh.name());
    let base_id = raw_name.map(sanitize_name).unwrap_or_default();
    let id = if base_id.is_empty() {
        unique_id(&format!("node-{}", node.index()), used)
    } else {
        unique_id(&base_id, used)
    };
    // The un-sanitized source name is kept as the label (SPEC.md §3).
    let label = raw_name.unwrap_or("").to_string();

    let mut triangles: Vec<[Vec3; 3]> = Vec::new();
    let mut color: Option<[u8; 3]> = None;
    for (i, primitive) in mesh.primitives().enumerate() {
        triangles.extend(primitive_triangles(&primitive, world, buffers));
        if i == 0 && primitive.material().index().is_some() {
            // First material's base color factor -> render tint (SPEC.md §3.2).
            let factor = primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_factor();
            color = Some(factor_to_rgb(factor));
        }
    }
    WorldObject::new(id, label, color, Geometry::Mesh { triangles })
}

/// World-space triangles of one primitive, honoring the draw mode (indexed
/// or sequential vertices; strips and fans expand to triangle lists). Points
/// and line modes contribute no triangles.
fn primitive_triangles(
    primitive: &gltf::Primitive,
    world: &Mat4,
    buffers: &[gltf::buffer::Data],
) -> Vec<[Vec3; 3]> {
    let reader =
        primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| data.0.as_slice()));
    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .map(|it| it.collect())
        .unwrap_or_default();
    let indices: Vec<u32> = reader
        .read_indices()
        .map(|it| it.into_u32().collect())
        .unwrap_or_default();
    // Non-indexed primitives address their vertices sequentially.
    let indices: Vec<u32> = if indices.is_empty() {
        (0..positions.len() as u32).collect()
    } else {
        indices
    };

    let vertex = |i: u32| -> Option<Vec3> {
        positions
            .get(i as usize)
            .map(|p| Vec3::new(p[0], p[1], p[2]))
    };
    let local: Vec<[Vec3; 3]> = match primitive.mode() {
        gltf::mesh::Mode::Triangles => indexed_triangles(&indices, vertex),
        gltf::mesh::Mode::TriangleStrip => strip_triangles(&indices, vertex),
        gltf::mesh::Mode::TriangleFan => fan_triangles(&indices, vertex),
        // Points, lines, and line loops are not surfaces (SPEC.md §3.2).
        _ => Vec::new(),
    };
    local
        .into_iter()
        .map(|tri| tri.map(|v| transform_point(world, v)))
        .collect()
}

/// Transform a point by an affine node matrix. Node transforms keep w = 1, so
/// this is the standard homogeneous divide guarded against w = 0.
fn transform_point(m: &Mat4, p: Vec3) -> Vec3 {
    let h = m.transform4([p.x, p.y, p.z, 1.0]);
    let inv_w = if h[3] == 0.0 {
        f32::INFINITY
    } else {
        1.0 / h[3]
    };
    Vec3::new(h[0] * inv_w, h[1] * inv_w, h[2] * inv_w)
}

/// Triangles from an indexed triangle list; triangles referencing missing
/// vertices (malformed files) are skipped, never panicking.
fn indexed_triangles(indices: &[u32], vertex: impl Fn(u32) -> Option<Vec3>) -> Vec<[Vec3; 3]> {
    let mut out = Vec::new();
    for chunk in indices.chunks_exact(3) {
        if let (Some(a), Some(b), Some(c)) = (vertex(chunk[0]), vertex(chunk[1]), vertex(chunk[2]))
        {
            out.push([a, b, c]);
        }
    }
    out
}

fn strip_triangles(indices: &[u32], vertex: impl Fn(u32) -> Option<Vec3>) -> Vec<[Vec3; 3]> {
    let mut out = Vec::new();
    for window in indices.windows(3) {
        if let (Some(a), Some(b), Some(c)) =
            (vertex(window[0]), vertex(window[1]), vertex(window[2]))
        {
            // Strip winding alternates; raycast and the renderer never
            // backface-cull, so keep the simple 3-window expansion.
            out.push([a, b, c]);
        }
    }
    out
}

fn fan_triangles(indices: &[u32], vertex: impl Fn(u32) -> Option<Vec3>) -> Vec<[Vec3; 3]> {
    let mut out = Vec::new();
    if let Some(pivot) = indices.first().and_then(|i| vertex(*i)) {
        for window in indices[1..].windows(2) {
            if let (Some(b), Some(c)) = (vertex(window[0]), vertex(window[1])) {
                out.push([pivot, b, c]);
            }
        }
    }
    out
}

/// Sanitize a node/mesh name into a world id (SPEC.md §3): lowercase,
/// whitespace and underscores become `-`, any other character outside
/// `[a-z0-9-]` is stripped. An empty result makes the caller fall back to
/// `node-<index>`.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| {
            if c.is_whitespace() || c == '_' {
                '-'
            } else {
                c
            }
        })
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
        .collect()
}

/// Reserve `base` or its first free `-2`/`-3`/... variant. The set is finite,
/// so the search always terminates; the HashSet is used for membership only
/// (never iterated) to keep outputs deterministic (SPEC.md §1).
fn unique_id(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{base}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// Map a 0..1 base color factor to u8: x255, rounded, clamped (SPEC.md §3.2).
fn factor_to_rgb(factor: [f32; 4]) -> [u8; 3] {
    let channel = |c: f32| -> u8 { (c * 255.0).round().clamp(0.0, 255.0) as u8 };
    [channel(factor[0]), channel(factor[1]), channel(factor[2])]
}

// ---------------------------------------------------------------------------
// tests — hand-computed expectations only

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_name_lowercases_and_dashes() {
        assert_eq!(sanitize_name("Test Cube"), "test-cube");
        assert_eq!(sanitize_name("My_Node"), "my-node");
        assert_eq!(sanitize_name("Big Sofa!"), "big-sofa");
        assert_eq!(sanitize_name("node 99"), "node-99");
        assert_eq!(sanitize_name("Ünicorn"), "nicorn");
        assert_eq!(sanitize_name("!!!"), "");
    }

    #[test]
    fn unique_id_appends_numeric_suffixes() {
        let mut used = HashSet::new();
        assert_eq!(unique_id("test-cube", &mut used), "test-cube");
        assert_eq!(unique_id("test-cube", &mut used), "test-cube-2");
        assert_eq!(unique_id("test-cube", &mut used), "test-cube-3");
        // A literal "-2" name never shadows the suffix sequence.
        assert_eq!(unique_id("other", &mut used), "other");
        assert_eq!(unique_id("test-cube-2", &mut used), "test-cube-2-2");
    }

    #[test]
    fn factor_to_rgb_rounds_and_clamps() {
        assert_eq!(factor_to_rgb([0.0, 0.5, 1.0, 1.0]), [0, 128, 255]);
        assert_eq!(factor_to_rgb([1.0, 1.0, 1.0, 1.0]), [255, 255, 255]);
        // Out-of-range factors clamp instead of wrapping.
        assert_eq!(factor_to_rgb([2.0, -1.0, 0.004, 1.0]), [255, 0, 1]);
    }

    #[test]
    fn node_matrix_trs_orders_scale_first() {
        let yaw90 = Quat::from_axis_angle_deg(Vec3::new(0.0, 1.0, 0.0), 90.0);
        let t = gltf::scene::Transform::Decomposed {
            translation: [0.0, 1.0, 0.0],
            rotation: [yaw90.x, yaw90.y, yaw90.z, yaw90.w],
            scale: [2.0, 1.0, 1.0],
        };
        let m = node_matrix(t);
        let p = transform_point(&m, Vec3::new(1.0, 0.0, 0.0));
        assert!((p.x - 0.0).abs() < 1e-6, "x {p:?}");
        assert!((p.y - 1.0).abs() < 1e-6, "y {p:?}");
        assert!((p.z + 2.0).abs() < 1e-6, "z {p:?}");
    }

    #[test]
    fn transform_point_applies_non_uniform_scale() {
        let t = gltf::scene::Transform::Decomposed {
            translation: [1.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 3.0, 5.0],
        };
        let m = node_matrix(t);
        let p = transform_point(&m, Vec3::new(0.0, 1.0, 1.0));
        assert!((p.x - 1.0).abs() < 1e-6);
        assert!((p.y - 3.0).abs() < 1e-6);
        assert!((p.z - 5.0).abs() < 1e-6);
    }

    #[test]
    fn indexed_triangles_skip_missing_vertices() {
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ];
        let vertex = |i: u32| -> Option<Vec3> {
            positions
                .get(i as usize)
                .map(|p| Vec3::new(p[0], p[1], p[2]))
        };
        let tris = indexed_triangles(&[0, 1, 2, 1, 3, 2, 0, 9, 1], vertex);
        // The last triangle references vertex 9, which does not exist.
        assert_eq!(tris.len(), 2);
    }

    #[test]
    fn strip_and_fan_expand_to_triangles() {
        let vertex = |i: u32| -> Option<Vec3> { Some(Vec3::new(i as f32, 0.0, 0.0)) };
        let strip = strip_triangles(&[0, 1, 2, 3], vertex);
        assert_eq!(strip.len(), 2);
        let fan = fan_triangles(&[0, 1, 2, 3], vertex);
        assert_eq!(fan.len(), 2);
        assert_eq!(fan[0][0], Vec3::new(0.0, 0.0, 0.0));
    }
}

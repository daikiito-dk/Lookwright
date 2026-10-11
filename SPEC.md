# Lookwright Specification — v0.1.0 (SSOT)

Normative. When code and this document disagree, fix the code. If this document
is wrong, change it in the same commit as the code and say why in the body.

> **Lookwright** — *Headless Perception & 6DoF Motion Test Framework for Smart
> Glasses & Spatial AI.* Playwright for spatial apps: define how a wearer's head
> and eyes move, then assert what they should see — with no headset and no GPU.

- Status: v0.1.0 — formats carry `"version": 1` internally.
- Product name: **Lookwright**. CLI binary: `lookwright`. npm wrapper: `npx lookwright`.

---

## 1. Conventions

- **JSON**: UTF-8, snake_case keys, no trailing commas. **YAML** is accepted
  for trajectories only (identical schema, parsed with `serde_yaml`).
- **Units in field names**: `_m` meters, `_sec` seconds, `_deg` degrees, `_hz`
  hertz, `_px` pixels. Ratios end in `_ratio`; coverage thresholds are
  `min_coverage` (0..1). Unitless vectors are named by frame (`position_m`,
  `forward` direction).
- **Coordinate system**: right-handed, meters. +X right, +Y up. Camera space
  follows OpenGL convention: camera looks down −Z, +X right, +Y up.
- **Quaternions**: `[x, y, z, w]`, normalized on read.
- **Euler**: `rotation_euler_ypr_deg: [yaw, pitch, roll]`, applied as
  `R = Ry(yaw) · Rx(pitch) · Rz(roll)` (degrees). Positive yaw turns left
  (−Z → −X); positive pitch looks up (−Z → +Y).
- **Gaze composition**: `camera_rotation = head_rotation · Ry(gaze_yaw) · Rx(gaze_pitch)`.
  Gaze angles are head-local.
- **Determinism**: same inputs on the same platform ⇒ byte-identical outputs.
  Text outputs are byte-stable across platforms. Raster images may differ by
  edge pixels across platforms (libm ULPs); golden comparison therefore uses
  thresholds (§9 A-05). Never introduce HashMap iteration, system time, or
  randomness into any serializer or test expectation.
- **No GPU**: CPU-only, dependency-light, Linux-CI-friendly by design.

## 2. Trajectory format

A trajectory describes 6DoF head motion, eye motion, and injected events over
time. File extension `.json` or `.yaml` (YAML accepted for this document only).

```json
{
  "version": 1,
  "duration_sec": 8.0,
  "head": [
    { "t_sec": 0.0, "position_m": [0.5, 1.70, 0.0],
      "rotation_euler_ypr_deg": [0, 0, 0] },
    { "t_sec": 3.0, "position_m": [0.5, 1.70, -2.2],
      "rotation_euler_ypr_deg": [70, 0, 0] }
  ],
  "gaze": [
    { "t_sec": 0.0, "yaw_deg": 0.0, "pitch_deg": 0.0 },
    { "t_sec": 3.0, "yaw_deg": 5.0, "pitch_deg": -8.0 }
  ],
  "events": [
    { "t_sec": 2.0, "type": "gaze_at", "target": "tv" },
    { "t_sec": 5.0, "type": "gesture", "name": "pinch" },
    { "t_sec": 6.5, "type": "voice", "transcript": "what am I looking at?" }
  ]
}
```

Rules:

- `head` is required and non-empty. Each sample: `t_sec`, `position_m`
  `[x,y,z]`, and a rotation — either `rotation_quat: [x,y,z,w]` or
  `rotation_euler_ypr_deg: [yaw,pitch,roll]` (degrees). Rotation may be
  **omitted** (identity). Both encodings on one sample is an error.
- `gaze` is optional; a missing `gaze` section means eyes centered for the
  whole run. `yaw_deg`/`pitch_deg` default to 0 and are head-local.
- `events` is optional. Event types v0.1: `gaze_at` (requires `target`),
  `gesture` (requires `name`), `voice` (requires `transcript`).
- `t_sec` must be strictly increasing within each array (`head`, `gaze`,
  `events`).
- Interpolation between samples: position linear, rotation SLERP (shortest
  arc), gaze angles linear. Before the first sample and after the last, values
  are held constant (clamp).
- `duration_sec` is optional. The run duration is `max(t_sec)` over all
  sections unless `duration_sec` is given, and it must be ≥ that maximum.
- Validation codes (§7):
  - `T-01` head section missing or empty (path `/head`)
  - `T-02` `t_sec` not strictly increasing within a section
  - `T-03` rotation field present but invalid (wrong length, non-finite, zero
    quaternion), or both encodings present on one sample. Omitted rotation is
    identity, not an error.
  - `T-04` missing or non-finite numeric field (`t_sec`, `position_m`, gaze
    angles, event times, `duration_sec`), or a section that must be an array
    is not one
  - `T-05` event missing `type`, unknown `type`, missing its required
    parameter (`gaze_at`→`target`, `gesture`→`name`, `voice`→`transcript`),
    or a non-array `events` section
  - `T-06` `duration_sec` smaller than the largest sample/event time

## 3. World format

A world is a set of labeled objects. JSON only (YAML not accepted here).

```json
{
  "version": 1,
  "name": "Living Room",
  "objects": [
    {
      "id": "sofa",
      "label": "Sofa",
      "color": "#7a5230",
      "geometry": {
        "box": {
          "center_m": [0.0, -0.25, -2.5],
          "size_m": [2.2, 0.9, 0.9],
          "rotation_euler_ypr_deg": [0, 0, 0]
        }
      }
    },
    {
      "id": "scan-cloud",
      "label": "Lidar scan",
      "geometry": { "points_m": [[0,0,0], [0.1,0,0], [0,0.1,0.05]] }
    }
  ]
}
```

Rules:

- `id`: unique, non-empty, `[a-z0-9_-]+` (lowercase). `label` is optional
  human-readable text. `color` is optional `#rrggbb`, used by the renderer.
- `geometry` is exactly one of:
  - `box`: `center_m`, `size_m` (each component > 0), optional
    `rotation_euler_ypr_deg` (OBB; default identity).
  - `points_m`: array of `[x,y,z]` (min 1 point). AABB is the min/max of the
    points; the object is treated as its AABB for raycast/visibility and is
    rendered as its AABB (points are not rasterized individually in v0.1).
- Axis-aligned bounding box (AABB) = world-space AABB of the object (for a
  rotated box: the AABB of its 8 transformed corners).
- Validation codes (§7):
  - `W-01` object missing `id` / duplicate `id` / invalid `id` characters
  - `W-02` geometry missing or invalid (neither `box` nor `points_m`, wrong
    field types)
  - `W-03` `box.size_m` has a component ≤ 0
  - `W-04` non-finite number in the world
  - `W-05` `points_m` empty

### 3.1 Built-in preset worlds

Embedded at compile time; referenced by id in configs:
`living_room`, `office`, `urban_street`, `warehouse` (contents live in
`crates/lookwright-perception/assets/presets/*.json` and follow §3 exactly).
Unknown preset ids are `W-07`.

### 3.2 glTF 2.0 import

- `.gltf` and `.glb` are accepted via the `gltf` crate; parse failures surface
  as `P-01` with the file path.
- Every named scene node with a mesh becomes one object. `id` = node name
  (fallback: mesh name, then `node-<index>`), sanitized to §3 rules
  (lowercased, spaces→`-`); collisions get `-2`, `-3` suffixes.
- Object geometry = world-space triangles of all mesh primitives (node
  transforms applied). AABB from transformed vertices.
- Raycast uses triangle intersection for glTF meshes; JSON-world boxes use
  oriented-box intersection; points objects use AABB intersection.
- Base color factor of the first material (if any) is used as the render tint.
- OpenUSD and PLY are roadmap (§16), not in v0.1.

## 4. Camera model

```
camera: { "fov_y_deg": 70.0, "near_m": 0.05, "far_m": 50.0 }
```

- Perspective projection, vertical FOV in degrees (1.0 < fov < 179), `near_m`,
  `far_m` (0 < near < far). Defaults shown above. `aspect` is derived as
  `width_px / height_px`.
- View matrix = inverse of the camera world pose (rigid inverse: transpose
  rotation, negate transformed translation).
- Clip space z ∈ [−1, 1] (OpenGL mapping):
  `P = [[f/aspect,0,0,0],[0,f,0,0],[0,0,(far+near)/(near−far), 2·far·near/(near−far)],[0,0,−1,0]]`,
  `f = 1/tan(fov_y_rad/2)`.
- Screen mapping for `resolution_px: [w, h]`:
  `px = (ndc_x · 0.5 + 0.5) · w`, `py = (0.5 − ndc_y · 0.5) · h`.
- Points with clip `w ≤ 0` do not project (`project` returns `None`).
- Validation code: `C-03` invalid camera parameters (fov out of range,
  `near_m`/`far_m` non-positive or inverted, non-finite).

## 5. UI anchors

UI elements are defined inside a test config (§6). Example:

```json
{
  "id": "info-card",
  "size_m": [0.40, 0.25],
  "color": "#00ccff",
  "anchor": { "mode": "world", "position_m": [-1.2, 1.5, -3.5],
              "rotation_euler_ypr_deg": [0, 0, 0], "billboard": false },
  "wobble": { "amplitude_m": 0.05, "frequency_hz": 1.0 }
}
```

- `anchor.mode`: `world` (position fixed in the world) or `head`
  (position = camera pose applied to `offset_m`, i.e. HUD-style).
- `color` is optional `#rrggbb` (default `#00ccff`), tinting the panel in
  golden frames (§12). Malformed values are `W-06`. (Parsed by the CLI config
  layer; the core `UiSpec` struct does not carry it.)
- `billboard: true` (world mode only) re-faces the panel toward the camera
  every frame. Default `false`. Head-anchored panels always face the camera by
  construction.
- `wobble` is **fault injection**. The placement offset applied to the
  expected position is `amplitude_m · sin(2π · frequency_hz · t_sec)` on each
  of x, y, z independently (max drift = `√3 · amplitude_m`). `frequency_hz`
  must be > 0. It exists so the `anchor_drift` assertion is demonstrable; do
  not use it in passing examples with goldens.
- Placement metrics per sample:
  - expected center = anchor rule applied exactly;
  - actual center = placement actually used (expected + wobble);
  - `drift_m` = `|actual − expected|`;
  - corners = center ± half-size along the panel's axes, ordered
    [+x+y, −x+y, −x−y, +x−y];
  - visibility: the quad is sampled on a 4×4 grid (`u, v ∈ {0, 1/3, 2/3, 1}`,
    row-major from the +x+y corner); coverage = fraction of grid points that
    project into the viewport;
  - occlusion: `occluded = true` if a scene raycast from the camera to the
    panel center hits a scene object strictly closer than the panel.
- Validation codes: UI ids must be non-empty; duplicates and dangling
  references are `C-02`. Invalid UI/anchor fields (unknown mode, missing or
  malformed `position_m`/`offset_m`/`size_m`, bad `wobble`) are `W-06`.

## 6. Test config format

```json
{
  "version": 1,
  "name": "walk-and-look",
  "target": "ray-ban-meta",
  "world": "living_room",
  "trajectory": "examples/trajectories/walk_and_look_left.json",
  "camera": { "fov_y_deg": 70.0, "near_m": 0.05, "far_m": 50.0 },
  "resolution_px": [640, 400],
  "sample_hz": 30.0,
  "ui": [ /* §5 */ ],
  "assertions": [
    { "type": "ui_visible", "ui": "info-card", "duration_sec": 2.0,
      "min_coverage": 0.5 },
    { "type": "gaze_focused", "target": "tv", "duration_sec": 1.0 },
    { "type": "anchor_drift", "ui": "info-card", "max_drift_m": 0.01 },
    { "type": "event_fired", "event": "gesture", "name": "pinch" },
    { "type": "golden_visual",
      "dir": "examples/golden/walk_and_look",
      "times_sec": [1.0, 4.0],
      "max_mean_abs_diff": 1.5, "max_changed_pixel_ratio": 0.02 }
  ]
}
```

- `world`: preset id (§3.1) or a path to a world JSON. `trajectory`: path.
  Paths are resolved relative to the config file.
- `target` is optional metadata recorded in the report; v0.1 accepted values:
  `ray-ban-meta`, `orion`, `horizon-os`, `generic`. It never changes simulation
  behavior — Lookwright is always headless.
- `sample_hz` default 30.0. Samples run at `t = 0, dt, 2·dt, … ≤ duration`.
- Validation codes: `C-01` missing/invalid `world`, `trajectory`, or `target`;
  `C-02` assertion/UI references an unknown id, duplicate UI id, or empty UI id;
  `C-03` invalid camera; `C-04` invalid `sample_hz`/`resolution_px`;
  `C-05` unknown assertion type or bad assertion params;
  `C-06` bad `golden` params (e.g. empty `times_sec`).

## 7. Validation codes (public API)

| Code | Domain | Meaning |
|------|--------|---------|
| P-01 | any document | unreadable or not valid for its format (JSON/YAML/glTF); message includes the parser output, path names the file or `/` |
| T-01…T-06 | trajectory | §2 |
| W-01…W-06 | world / UI anchors | §3, §5 |
| W-07 | worlds | unknown preset id (§3.1) |
| C-01…C-06 | test config | §6 |
| A-01…A-05 | assertions | §9 |

Validation failures always produce: a stable code, a JSON Pointer path into the
document, and a message naming the offending value. Adding codes is
non-breaking; renumbering or redefining is breaking.

## 8. Perception feed

Captured per time step; plain JSON or JSON-LD (`--format jsonld`).

```json
{
  "schema": "lookwright/perception-frame@1",
  "t_sec": 2.5,
  "camera": {
    "position_m": [0.5, 1.7, -1.1],
    "forward": [-0.94, -0.14, -0.31],
    "gaze_target": "tv"
  },
  "visible_objects": [
    { "id": "tv", "label": "TV",
      "distance_m": 2.84,
      "bbox_px": [180.5, 122.0, 420.25, 288.75],
      "world_aabb_m": { "min_m": [-1.9, 1.1, -3.6], "max_m": [-0.4, 2.0, -3.4] } }
  ]
}
```

- `visible_objects` = objects whose AABB intersects the view frustum, sorted
  by `distance_m` ascending, ties by id. `distance_m` = `|camera_position −
  AABB center|`. `gaze_target` = id of the nearest gaze-ray hit, or `null`.
- `bbox_px` = `[x0, y0, x1, y1]` pixel bounds of the projected AABB corners;
  `null` if any corner fails to project (`w ≤ 0`).
- `label` falls back to the id when absent. `bbox_px` and `world_aabb_m` use
  exact f32 serialization (Rust default float formatting).
- JSON-LD adds an inline `@context` (terms `PerceptionFrame`, `SpatialObject`,
  `visible_objects`, `gaze_target`, `bbox_px`, `distance_m`, `t_sec`) with
  `@type: "PerceptionFrame"` on the frame and `@type: "SpatialObject"` per
  object. This feed is the mock input for contextual-AI agents (Llama vision
  etc.).

## 9. Assertion catalog (public API)

| Code | `type` | Passes iff | Key params (defaults) |
|------|--------|------------|----------------------|
| A-01 | `ui_visible` | cumulative time the UI is visible ≥ `duration_sec`; visible = coverage ≥ `min_coverage` over the 16-point grid and, if `require_unoccluded`, the center is not occluded | `ui`, `duration_sec` (1.0), `min_coverage` (0.5), `require_unoccluded` (false) |
| A-02 | `gaze_focused` | cumulative time the gaze ray's nearest hit is `target` ≥ `duration_sec` | `target`, `duration_sec` (1.0) |
| A-03 | `anchor_drift` | max `drift_m` over samples ≤ `max_drift_m` | `ui`, `max_drift_m` (0.01) |
| A-04 | `event_fired` | an event of type `event` (and `name`, if given) fired within the run | `event`, optional `name` |
| A-05 | `golden_visual` | for every `t` in `times_sec`: `dir/frame_<t_millis>.png` exists and `mean_abs_diff ≤ max_mean_abs_diff` and `changed_pixel_ratio ≤ max_changed_pixel_ratio` | `dir`, `times_sec`, `max_mean_abs_diff` (1.5), `max_changed_pixel_ratio` (0.02) |

- Cumulative-time assertions use the sample grid; a sample contributes its
  `dt`. Timing is at sample granularity only in v0.1.
- Assertion evaluation order = config order. All assertions always run; the CLI
  reports the full table and exits with the worst result.

## 10. CLI contract

```
lookwright test   --config <path> [--report <path>] [--junit <path>]
lookwright run    (alias of test; also accepts --headless (no-op, always true)
                  and --target <id>)
lookwright golden update --config <path>
lookwright perception --world <id|path> --trajectory <path>
                      [--at-sec <t>] [--format json|jsonld] [--out <path>]
lookwright worlds list | worlds show <id>
lookwright validate --trajectory <path> | --world <path> | --config <path>
lookwright --version | --help
```

- `test` runs §6 configs, prints a human table, writes report JSON and JUnit
  XML when paths are given, and renders golden comparisons (A-05).
- `golden update` re-renders every `times_sec` frame to `dir` (creating it).
- Exit codes: `0` all assertions pass; `1` one or more assertions failed;
  `2` usage, IO, or validation error (with code + path + value).
- Human output goes to stdout, diagnostics to stderr. `--report` JSON and
  `--junit` XML are byte-deterministic (no timestamps; config name only).

## 11. Report formats

Report JSON (`--report`):

```json
{
  "schema": "lookwright/report@1",
  "name": "walk-and-look",
  "target": "ray-ban-meta",
  "world": "living_room",
  "trajectory": "examples/trajectories/walk_and_look_left.json",
  "samples": 240,
  "assertions": [
    { "code": "A-01", "type": "ui_visible", "pass": true,
      "detail": "visible 6.90s of 8.00s (required 2.00s, coverage 0.94)",
      "metrics": { "visible_sec": 6.9, "mean_coverage": 0.94 } }
  ],
  "summary": { "pass": 4, "fail": 0, "total": 4, "exit_code": 0 }
}
```

JUnit XML (`--junit`): one `<testsuite name="lookwright: <config name>">`
containing one `<testcase name="<code>: <type>[ <id>]">` per assertion;
failures carry the detail message. No timestamps in v0.1 (byte-determinism).

## 12. Renderer contract

- CPU rasterizer only. Input: mesh batches (world-space triangles + RGB color
  per batch), camera (§4), `resolution_px`. Boxes are converted by the caller
  (perception crate) into 12 triangles.
- Golden frames render every world object batch **and** every UI panel quad
  (two triangles per panel, tinted with the UI `color`, §5) so visual
  regression covers spatial-UI anchoring, not just the scene.
- Z-buffered triangle rasterization, top-left fill rule, no filtering/AA.
- Outputs: RGB8 buffer and linear depth (`depth01` = NDC z mapped from
  [−1,1] to [0,1]).
- PNG: `frame_<t_millis>.png` (RGB8) and optional `frame_<t_millis>.depth.png`
  (16-bit grayscale, `depth01 · 65535`, saturating).
- Diff metrics between two RGB8 images of identical size:
  `mean_abs_diff` (mean per-channel |a−b|, 0..255 scale), `changed_pixel_ratio`
  (fraction of pixels with max-channel |a−b| > 1), `max_abs_diff`.
- Diff requires identical dimensions; mismatched sizes are an error.

## 13. Crate API contract (v0.1.0)

Key signatures agents and integrators rely on (exact names matter):

```rust
// lookwright-core (SPEC §1–§5)
pub struct Issue { pub code: &'static str, pub path: String, pub message: String }
pub struct Vec3 { pub x: f32, pub y: f32, pub z: f32 }
pub struct Quat { pub x: f32, pub y: f32, pub z: f32, pub w: f32 }
pub struct Mat4 { /* column-major: m[col*4 + row] */ }
pub struct CameraPose { pub position_m: Vec3, pub rotation: Quat }
pub struct CameraParams { pub fov_y_deg: f32, pub near_m: f32, pub far_m: f32,
                          pub aspect: f32 }
impl CameraPose { pub fn view_matrix(&self) -> Mat4; pub fn forward(&self) -> Vec3 }
impl CameraParams { pub fn perspective_matrix(&self) -> Mat4 }
pub struct Projector { /* view-projection + resolution */ }
impl Projector {
    pub fn new(camera: &CameraPose, params: &CameraParams, res_px: [u32; 2]) -> Self;
    pub fn project(&self, p: Vec3) -> Option<[f32; 2]>;
    pub fn vp_matrix(&self) -> Mat4;
}
impl Trajectory {
    pub fn parse(text: &str) -> Result<Self, Issue>;   // JSON or YAML
    pub fn sample(&self, t_sec: f32) -> Sample;
    pub fn duration_sec(&self) -> f32;
    pub fn events(&self) -> &[Event];
    pub fn events_between(&self, lo_sec: f32, hi_sec: f32) -> Vec<&Event>;
}
pub struct Sample { pub t_sec: f32, pub camera: CameraPose,
                    pub gaze_yaw_deg: f32, pub gaze_pitch_deg: f32 }
pub enum AnchorMode { World, Head }
pub enum Anchor {
    World { position_m: Vec3, rotation_euler_ypr_deg: [f32; 3], billboard: bool },
    Head { offset_m: Vec3 },
}
pub struct UiSpec { pub id: String, pub size_m: [f32; 2], pub anchor: Anchor,
                    pub wobble: Option<Wobble> }
impl UiSpec {
    pub fn from_value(v: &serde_json::Value, path: &str) -> Result<Self, Issue>;
    pub fn place(&self, t_sec: f32, camera: &CameraPose) -> UiPlacement;
}
pub struct UiPlacement { pub center_m: Vec3, pub expected_center_m: Vec3,
                         pub rotation: Quat, pub corners_m: [Vec3; 4],
                         pub drift_m: f32 }
impl UiPlacement { pub fn grid_points(&self, grid: usize) -> Vec<Vec3> }

// lookwright-perception (SPEC §3, §8)
impl World {
    pub fn parse(json: &str) -> Result<Self, Issue>;
    pub fn from_preset(id: &str) -> Result<Self, Issue>;      // W-07 unknown
    pub fn load(path: &Path) -> Result<Self, Issue>;          // .json/.gltf/.glb
    pub fn preset_ids() -> &'static [&'static str];
    pub fn name(&self) -> &str;
    pub fn objects(&self) -> &[WorldObject];
}
impl WorldObject {
    pub fn id(&self) -> &str; pub fn label(&self) -> &str;
    pub fn aabb(&self) -> (Vec3, Vec3);                       // (min_m, max_m)
    pub fn triangles_m(&self) -> Vec<[Vec3; 3]>;              // renderable
    pub fn color_rgb(&self) -> [u8; 3];                       // set color or id-hash
}
pub struct GazeHit { pub object_id: String, pub distance_m: f32, pub point_m: Vec3 }
pub fn cast_gaze(world: &World, origin_m: Vec3, dir: Vec3) -> Option<GazeHit>;
pub struct Frustum;
impl Frustum { pub fn from_vp(vp: Mat4) -> Self; pub fn intersects_aabb(&self, min_m: Vec3, max_m: Vec3) -> bool; }
pub struct PerceptionFrame;
impl PerceptionFrame {
    pub fn capture(t_sec: f32, camera: &CameraPose, world: &World,
                   projector: &Projector) -> Self;
    pub fn to_json(&self) -> String; pub fn to_jsonld(&self) -> String;
}

// lookwright-renderer (SPEC §12)
pub struct MeshBatch { pub triangles_m: Vec<[Vec3; 3]>, pub color: [u8; 3] }
pub fn render(batches: &[MeshBatch], camera: &CameraPose,
              params: &CameraParams, res_px: [u32; 2]) -> Frame;
pub struct Frame { pub width_px: u32, pub height_px: u32, pub rgb: Vec<u8>,
                   pub depth01: Vec<f32> }
pub fn write_rgb_png(path: &Path, frame: &Frame) -> std::io::Result<()>;
pub fn write_depth_png(path: &Path, frame: &Frame) -> std::io::Result<()>;
pub fn read_rgb_png(path: &Path) -> std::io::Result<Frame>;
pub struct DiffMetrics { pub mean_abs_diff: f32, pub max_abs_diff: u8,
                         pub changed_pixel_ratio: f32 }
pub fn diff(a: &Frame, b: &Frame) -> std::io::Result<DiffMetrics>;
```

## 14. SDK contract (sdk/ts)

- npm package `lookwright`; `npx lookwright …` forwards args to the native
  binary. Binary resolution order: `$LOOKWRIGHT_BIN` → `lookwright` on PATH →
  error with install instructions (no silent downloads in v0.1).
- `runTest(configPath, { bin, reportPath, junitPath })` →
  `{ exitCode: 0|1, report }` with `report` typed per §11 (exit code 2 raises).
- TS types mirror the JSON schemas exactly (field-for-field, same names).

## 15. Versioning

- Formats embed `"version": 1`. Anything that changes bytes on disk for equal
  inputs bumps this and the SPEC version together, with a CHANGELOG entry.
- Codes (§7) and exit codes (§10) follow the stability rules stated there.

## 16. Roadmap (explicitly not in v0.1)

GPU-accelerated offscreen rendering (Vulkan/WebGPU headless), OpenUSD import,
PLY/LAS point clouds, Horizon OS on-device harness, continuous (sub-sample)
assertion timing, per-pixel occlusion masks.

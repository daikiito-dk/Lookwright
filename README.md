# Lookwright

> **Headless Perception & 6DoF Motion Test Framework for Smart Glasses & Spatial AI.**

Playwright for spatial apps. Describe how a wearer's head and eyes move, then
assert what they should see — **no headset, no GPU, no display**. Everything
runs on a plain Linux CI box.

| | |
|---|---|
| Status | v0.1.0 — formats carry `"version": 1` ([SPEC.md](SPEC.md) is the SSOT) |
| Docs | [daikiito-dk.github.io/Lookwright](https://daikiito-dk.github.io/Lookwright/) |
| Language | Rust workspace + TypeScript SDK |
| Rendering | Deterministic CPU rasterizer (no Vulkan/WebGPU/Mesa anywhere) |
| License | MIT |

## Why

Testing smart-glasses apps — heads-up displays like Ray-Ban Display, AR
glasses like Snap Specs, Horizon OS headsets — normally means wearing the
device and walking around. Platform simulators help you try an app by hand,
but none of that gives you a repeatable pass/fail check in CI.
Lookwright replays head motion and gaze from a data file, renders the scene
and your spatial UI on the CPU, and turns "does the UI stay anchored while
the wearer walks and looks around?" into a repeatable test with a red/green
exit code. The same simulation emits a structured **perception feed**
(visible objects, bounding boxes, gaze target) that you can pipe straight into
a contextual AI agent (e.g. Llama vision) and unit-test its decisions.

```
┌────────────────────────────────────────────────────────┐
│             CLI / GitHub Actions Runner                │
│        lookwright test · golden · perception           │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│                    Lookwright Core                     │
│  - Trajectory Engine (6DoF head / eye motion)           │
│  - UI Anchor Engine (World Lock / Head Lock)           │
│  - Deterministic CPU Renderer (no GPU)                 │
│  - Spatial Perception Mock (BBox, Depth, Gaze Ray)     │
└────────────┬───────────────────────────┬───────────────┘
             │                           │
┌────────────▼─────────────┐ ┌────────────▼──────────────┐
│  Spatial UI under test   │ │ Contextual AI under test │
│  (anchors / layout)      │ │ (vision & gaze prompts)  │
└──────────────────────────┘ └───────────────────────────┘
```

## Quick start

```bash
git clone https://github.com/daikiito-dk/Lookwright && cd Lookwright

# Run the shipped example end to end (assertions + golden images)
cargo run -p lookwright-cli -- test --config examples/tests/walk_and_look.test.json

# Inspect the synthetic world
cargo run -p lookwright-cli -- worlds list
cargo run -p lookwright-cli -- worlds show living_room

# Dump the AI perception feed (mock input for Llama-style agents)
cargo run -p lookwright-cli -- perception \
  --world living_room \
  --trajectory examples/trajectories/walk_and_look_left.json \
  --at-sec 1.0 --format jsonld
```

Typical output of a passing run:

```
lookwright: walk-and-look
A-01  PASS  ui_visible info-card: visible 5.53s of 8.00s (required 2.00s, coverage 0.99)
A-02  PASS  gaze_focused tv: gaze on tv for 4.43s (required 1.00s)
A-03  PASS  anchor_drift info-card: max drift 0.0000m within 0.0100m
A-03  PASS  anchor_drift hud-hint: max drift 0.0000m within 0.0100m
A-04  PASS  event_fired pinch: event gesture fired at t=5.000s
A-04  PASS  event_fired voice: event voice fired at t=6.800s
A-05  PASS  golden_visual: 2 frame(s) within thresholds (worst mean_abs_diff 0.000, changed_pixel_ratio 0.00000)
summary: 7 pass, 0 fail
```

Exit codes (SPEC.md §10): `0` pass, `1` assertion failure, `2` usage or
validation error — so Lookwright drops straight into any CI.

## The three file types

1. **Trajectory** (`examples/trajectories/walk_and_look_left.json`) — 6DoF
   head keyframes (`t_sec`, `position_m`, rotation as quaternion or euler),
   head-local gaze angles, and injected events (`gaze_at`, `gesture`, `voice`).
   JSON or YAML.
2. **World** — a built-in preset (`living_room`, `office`, `urban_street`,
   `warehouse`), a JSON world of labeled boxes/point clouds, or any glTF 2.0
   scene (`.gltf`/`.glb`).
3. **Test config** (`examples/tests/walk_and_look.test.json`) — world +
   trajectory + camera + UI panels + assertions.

```json
{
  "world": "living_room",
  "trajectory": "../trajectories/walk_and_look_left.json",
  "ui": [
    { "id": "info-card", "size_m": [0.40, 0.25], "color": "#00ccff",
      "anchor": { "mode": "world", "position_m": [0.0, 1.9, -3.4] } }
  ],
  "assertions": [
    { "type": "ui_visible", "ui": "info-card", "duration_sec": 2.0 },
    { "type": "gaze_focused", "target": "tv", "duration_sec": 1.0 },
    { "type": "anchor_drift", "ui": "info-card", "max_drift_m": 0.01 },
    { "type": "golden_visual", "dir": "../golden/walk_and_look",
      "times_sec": [1.0, 5.0] }
  ]
}
```

## Assertions (SPEC.md §9)

| Code | `type` | Passes when |
|------|--------|-------------|
| A-01 | `ui_visible` | the panel stays visible for `duration_sec` (coverage ≥ `min_coverage`) |
| A-02 | `gaze_focused` | the gaze ray hits `target` for `duration_sec` |
| A-03 | `anchor_drift` | world/head-lock placement drift stays under `max_drift_m` |
| A-04 | `event_fired` | a gesture/voice/gaze event fired during the run |
| A-05 | `golden_visual` | rendered frames match golden PNGs within thresholds |

The `wobble` fault injection (SPEC.md §5) lets you prove A-03 catches bad
anchoring — see `examples/tests/faulty_anchor.test.json`, which intentionally
fails (`lookwright test` exits 1).

## Visual regression without a GPU

`lookwright golden update --config <cfg>` re-renders the golden frames
(`frame_<t_millis>.png`). `test` re-renders the same frames and compares with
`mean_abs_diff` / `changed_pixel_ratio` thresholds, so cross-platform libm
differences (macOS ARM vs Linux x86) never flake CI. Goldens are generated,
never hand-edited.

## Perception feed for contextual AI

```bash
lookwright perception --world living_room --trajectory walk.json \
  --at-sec 1.0 --format jsonld
```

```json
{
  "@type": "PerceptionFrame",
  "schema": "lookwright/perception-frame@1",
  "t_sec": 1.0,
  "camera": { "position_m": [0.0, 1.74, -0.6], "forward": [0.0, -0.0436, -0.9990],
              "gaze_target": "tv" },
  "visible_objects": [
    { "@type": "SpatialObject", "id": "tv", "label": "TV", "distance_m": 4.32,
      "bbox_px": [266.30, 186.86, 373.70, 246.78],
      "world_aabb_m": { "min_m": [-0.8, 0.85, -4.94], "max_m": [0.8, 1.75, -4.86] } }
  ]
}
```

Feed this into your agent's prompt pipeline and assert on its output — the
"what is the wearer looking at?" logic becomes a unit test.

**Scope:** Lookwright tests a model of your spatial UI (panels described in the
test config) and produces the input your AI would see. It does not run your
app on a device, and the worlds are synthetic, not camera footage.

## TypeScript SDK / npx

> **The npm package is not published yet.** Until it is, build the SDK from
> this repository and point it at the native binary with `$LOOKWRIGHT_BIN`:
>
> ```bash
> cargo build --release -p lookwright-cli
> (cd sdk/ts && npm ci && npm run build)
> LOOKWRIGHT_BIN="$PWD/target/release/lookwright" \
>   node sdk/ts/dist/src/cli.js test --config examples/tests/walk_and_look.test.json
> ```

Once published, usage will look like this:

```ts
import { runTest } from "lookwright";

const { exitCode, report } = runTest("spatial/walk_and_look.test.json", {
  reportPath: "report.json",
});
```

```bash
npx lookwright test --config spatial/walk_and_look.test.json --junit junit.xml
```

The wrapper resolves the native binary from `$LOOKWRIGHT_BIN`, then `lookwright`
on `PATH` (see `sdk/ts/README.md`).

## CI

- GitHub Actions: `.github/workflows/ci.yml` runs fmt + clippy + the full test
  suite + a self-test of the shipped example, and publishes the JUnit report.
- For other repos: `examples/ci-github-action/` contains a reusable composite
  action.
- Docker: `docker build -t lookwright .` (CPU-only image, `debian:bookworm-slim`
  runtime).

## Layout

```
crates/lookwright-core        6DoF trajectories, camera math, UI anchors
crates/lookwright-perception  worlds (presets, JSON, glTF), raycast, frustum, feed
crates/lookwright-renderer    deterministic CPU rasterizer, PNG, image diff
crates/lookwright-cli         the `lookwright` binary
sdk/ts                        TypeScript SDK + npx wrapper
examples/                     trajectories, test configs, goldens, CI action
SPEC.md                       the single source of truth (formats + codes)
AGENTS.md                     rules for AI coding agents working in this repo
```

## Development

```bash
cargo test --workspace
cargo fmt --all && cargo clippy --all-targets -- -D warnings
(cd sdk/ts && npm ci && npm test)
```

`SPEC.md` is normative: when code and spec disagree, fix the code. Validation
codes (`T-*`, `W-*`, `C-*`, `P-01`) and assertion codes (`A-01`…`A-05`) are
public API. See `AGENTS.md` for the full working agreement.

## Roadmap (SPEC.md §16)

GPU-accelerated offscreen rendering (headless Vulkan/WebGPU), OpenUSD and
PLY/LAS point-cloud import, a Horizon OS on-device harness, sub-sample
assertion timing, per-pixel occlusion masks.

# Changelog

## 0.1.0 (2026-10-11)

Initial release.

- 6DoF trajectory engine: JSON/YAML head + eye motion, event injection,
  SLERP/lerp interpolation with clamped holds (`T-01`…`T-06`).
- Spatial UI anchors: world-lock / head-lock / billboard placement, wobble
  fault injection, drift metrics (`W-06`).
- Synthetic worlds: built-in presets (living room, office, urban street,
  warehouse), JSON world format, glTF 2.0 import (`W-01`…`W-07`).
- Perception feed: frustum culling, gaze raycast, 2D/3D bounding boxes, JSON
  and JSON-LD output for contextual-AI agents.
- Deterministic CPU rasterizer (no GPU): RGB + depth PNG, golden-image visual
  regression with thresholds.
- `lookwright` CLI: `test`, `run`, `golden update`, `perception`, `worlds`,
  `validate`; exit codes 0/1/2; JSON report + JUnit XML.
- Assertion catalog A-01…A-05 (`ui_visible`, `gaze_focused`, `anchor_drift`,
  `event_fired`, `golden_visual`).
- TypeScript SDK + `npx lookwright` wrapper (zero runtime dependencies).
- GitHub Actions workflow, Docker image, reusable CI action example.

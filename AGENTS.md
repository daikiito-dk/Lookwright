# AGENTS.md — Guidance for Factory Droid and other coding agents

This file tells autonomous agents how to work in this repository. `SPEC.md` is
the single source of truth; when code and `SPEC.md` disagree, fix the code (or,
if the spec itself is wrong, change `SPEC.md` in the same commit and record why).

## Repository layout

```
Lookwright/
├── AGENTS.md                 # this file
├── SPEC.md                   # normative formats, codes, CLI/report contracts (SSOT)
├── Cargo.toml                # workspace root
├── crates/
│   ├── lookwright-core/      # scalar math, 6DoF trajectory engine, camera, UI anchors
│   ├── lookwright-perception/# worlds (presets, JSON, glTF), raycast, frustum, perception feed
│   ├── lookwright-renderer/  # deterministic CPU rasterizer, PNG, image diff
│   └── lookwright-cli/       # `lookwright` binary: test / golden / perception / worlds / validate
├── sdk/ts/                   # @lookwright TS SDK + `npx lookwright` wrapper
├── examples/                 # trajectories, test configs, golden images, CI snippets
├── .github/workflows/ci.yml  # fmt + clippy + test + SDK + self-test
└── Dockerfile                # headless Linux image for self-hosted runners
```

## Build and test commands

```bash
cargo test --workspace        # all unit + integration tests
cargo fmt --all               # run before committing
cargo clippy --all-targets -- -D warnings
cargo run -p lookwright-cli -- test --config examples/tests/walk_and_look.test.json
cargo run -p lookwright-cli -- golden update --config examples/tests/walk_and_look.test.json
(cd sdk/ts && npm ci && npm run build && npm test)
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy, tests, the TS SDK suite, and a
self-test that runs the example config through the CLI and uploads the JUnit
report. All gates must pass; do not weaken a test to make it green.

## Hard rules

1. **SPEC.md is the SSOT.** Every file format, field name, validation code,
   assertion code, exit code, and report schema lives in `SPEC.md`. Change the
   spec and the implementation in the same commit.
2. **No GPU.** The entire stack is CPU, headless, and dependency-light. No
   Vulkan/WebGPU/Mesa/EGL anywhere. GPU-accelerated rendering is roadmap only
   (SPEC.md §16) and must never become a test dependency.
3. **Determinism is a contract.** Text outputs (reports, JUnit XML, perception
   JSON) are byte-identical across runs. Raster images are compared with
   thresholds (SPEC.md §9, A-05) because libm may differ by 1 ULP across
   platforms; never tighten a default threshold without bumping the spec
   version. Never introduce HashMap iteration, system time, or randomness into
   any serializer or test expectation.
4. **Codes are public API.** Validation codes `T-01…`, `W-01…`, `C-01…` and
   assertion codes `A-01…A-05` are defined in `SPEC.md`. Renumbering or changing
   the meaning of a code is a breaking change. Adding a rule means: spec entry +
   implementation + fixture + doc table, in one commit.
5. **Errors state path and value.** Validation issues carry a JSON Pointer path
   and a message naming the offending value. Library code never panics; return
   `Result`/`Option`.
6. **Units are in field names.** `t_sec`, `position_m`, `yaw_deg`, `fov_y_deg`,
   `sample_hz`, `resolution_px`, `duration_sec`, `max_drift_m` — never
   introduce unitless numeric names. Ratios end in `_ratio`; coverage is
   `min_coverage` (0..1).
7. **Golden files are generated.** Images under `examples/golden/` exist only
   via `lookwright golden update`. Never hand-edit, never regenerate from a
   developer laptop when CI can do it — delete and regenerate.
8. **CLI contract.** Exit codes: `0` pass, `1` assertion failure, `2` usage or
   validation error. Keep this exact mapping covered in
   `crates/lookwright-cli/tests/cli.rs`.
9. **Commit style.** Imperative subject line ≤ 72 chars, body explains why. One
   logical change per commit. Never commit `target/`, `node_modules/`,
   `dist/`, or stray report artifacts.
10. **TDD.** Write the failing test first (or in the same change). Every module
    ships with unit tests; behavior changes without tests are rejected.

## Definition of done for any change

- [ ] `cargo test --workspace` green, including new fixtures
- [ ] `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` clean
- [ ] Golden images regenerated via the CLI if rendering or worlds changed
- [ ] `SPEC.md` updated if any format, field, code, default, or exit code changed
- [ ] Bilingual READMEs (`README.md` / `README.ja.md`) updated for user-facing changes

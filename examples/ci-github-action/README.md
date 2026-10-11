# Lookwright in GitHub Actions

Reusable setup for testing a smart-glasses app repo with Lookwright — fully
headless (CPU rendering, no GPU, no display, no headset).

## Option A: use the composite action

Reference this directory as a composite action from your own repository:

```yaml
# .github/workflows/spatial-tests.yml in YOUR repo
name: spatial-tests
on: [push, pull_request]
jobs:
  lookwright:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: daikiito-dk/Lookwright/examples/ci-github-action@main
        with:
          config: spatial/walk_and_look.test.json   # path in your repo
          junit: lookwright-junit.xml
```

The action installs the `lookwright` CLI with `cargo install`, runs
`lookwright test`, fails the job on assertion failures (exit 1), and uploads
the JUnit report as an artifact.

## Option B: inline steps

```yaml
- name: Install Lookwright
  run: cargo install --git https://github.com/daikiito-dk/Lookwright --bin lookwright lookwright-cli
- name: Spatial UI regression tests
  run: lookwright test --config spatial/walk_and_look.test.json --junit junit.xml
- uses: actions/upload-artifact@v4
  if: always()
  with:
    name: junit
    path: junit.xml
```

## Notes

- Exit codes (SPEC.md §10): `0` all assertions pass, `1` assertion failure,
  `2` usage/validation error. GitHub Actions marks non-zero exits as failures.
- Golden images (`golden_visual`, SPEC.md §9) are compared with thresholds,
  so Linux CI pixels differing from your laptop's by libm ULPs still pass.
  Regenerate goldens only with `lookwright golden update` (never hand-edit).
- For self-hosted runners without Rust, use the published Docker image:
  `docker run --rm -v "$PWD:/work" ghcr.io/daikiito-dk/lookwright:latest \
    test --config /work/spatial/walk_and_look.test.json`

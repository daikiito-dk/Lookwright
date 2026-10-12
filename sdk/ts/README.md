# lookwright — TypeScript SDK & CLI wrapper

[Lookwright](https://github.com/daikiito-dk/Lookwright) is a headless
perception and 6DoF motion test framework for smart glasses and spatial AI.
This package ships the `npx lookwright` wrapper and a typed `runTest()` helper
for the native Rust CLI. It has **zero runtime dependencies**; the native
binary is a separate install and is never downloaded for you.

## Prerequisites

Install the native `lookwright` CLI:

```bash
cargo install --git https://github.com/daikiito-dk/Lookwright lookwright-cli
```

## Install

> **Not published to npm yet.** Until it is, build from the repository and set
> `LOOKWRIGHT_BIN` to the native binary:
>
> ```bash
> cargo build --release -p lookwright-cli
> (cd sdk/ts && npm ci && npm run build)
> LOOKWRIGHT_BIN="$PWD/target/release/lookwright" node sdk/ts/dist/src/cli.js --help
> ```
>
> Set `LOOKWRIGHT_BIN` explicitly when the package is installed locally: with
> `npx`, `node_modules/.bin` comes first on `PATH`, so a bare `lookwright` can
> resolve to this wrapper instead of the native binary.

Once published:

```bash
npm install lookwright
```

## CLI wrapper

`npx lookwright` forwards every argument to the native binary (SPEC §10):

```bash
npx lookwright test --config examples/tests/walk_and_look.test.json \
  --report report.json --junit junit.xml
```

Exit codes: `0` all assertions pass, `1` one or more assertions failed,
`2` usage/validation error.

## SDK

```ts
import { runTest } from "lookwright";

const { exitCode, report } = runTest(
  "examples/tests/walk_and_look.test.json",
  { reportPath: "report.json", junitPath: "junit.xml" },
);

if (exitCode === 0) {
  console.log(`${report?.summary.total} assertions passed`);
}
```

- `runTest(configPath, opts)` spawns
  `lookwright test --config <configPath>`, adding `--report`/`--junit` when
  `reportPath`/`junitPath` are given.
- Returns `{ exitCode: 0 | 1, report }` where `report` is the parsed report
  JSON (SPEC §11) when `reportPath` was given and the file exists and parses;
  otherwise `report` is `null`.
- Throws when the binary cannot be spawned, or when the CLI exits `2`
  (usage/validation error) or crashes — the error message carries the CLI's
  stderr.

## Binary resolution

1. `opts.bin` (`runTest` only)
2. `$LOOKWRIGHT_BIN`
3. `lookwright` on `PATH`

If no binary is found, you get an error with install instructions. The
wrapper never downloads anything silently.

## Development

```bash
cd sdk/ts
npm install
npm test   # builds with tsc, then runs the node:test suite (no Rust needed)
```

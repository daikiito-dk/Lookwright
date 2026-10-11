#!/usr/bin/env node
// `npx lookwright …` wrapper: forwards every argument to the native binary
// (SPEC §14). Binary resolution: `$LOOKWRIGHT_BIN` → `lookwright` on PATH.
// No silent downloads in v0.1.
import { spawnSync } from "node:child_process";
import { resolveLookwrightBin } from "./runner.js";

const proc = spawnSync(resolveLookwrightBin(), process.argv.slice(2), {
  stdio: "inherit",
});

if (proc.error) {
  process.stderr.write(
    "lookwright binary not found on PATH. Install with `cargo install --git https://github.com/daikiito-dk/Lookwright lookwright-cli` or set LOOKWRIGHT_BIN.\n",
  );
  process.exit(2);
}

// 0 pass, 1 assertion failure, 2 usage/validation error (SPEC §10).
// A signal/crash (status === null) surfaces as 1.
process.exit(proc.status ?? 1);

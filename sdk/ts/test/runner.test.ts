import { after, test } from "node:test";
import assert from "node:assert/strict";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { findNativeOnPath, runTest } from "../src/runner.js";

// The suite never depends on the Rust binary: stubs stand in for the CLI.

// Canned reports (SPEC §11 shape) the stubs write to `--report`.
const PASS_REPORT =
  '{"schema":"lookwright/report@1","name":"stub","target":null,"world":"living_room","trajectory":"t.json","samples":3,"assertions":[{"code":"A-01","type":"ui_visible","pass":true,"detail":"ok","metrics":{"visible_sec":3}}],"summary":{"pass":1,"fail":0,"total":1,"exit_code":0}}';
const FAIL_REPORT =
  '{"schema":"lookwright/report@1","name":"stub","target":null,"world":"living_room","trajectory":"t.json","samples":3,"assertions":[{"code":"A-01","type":"ui_visible","pass":false,"detail":"ok","metrics":{"visible_sec":3}}],"summary":{"pass":0,"fail":1,"total":1,"exit_code":1}}';

const tmp = mkdtempSync(join(tmpdir(), "lookwright-sdk-"));

/** Write an executable Node script used as a stand-in for the Rust CLI. */
function writeExecutableStub(name: string, source: string): string {
  const stubPath = join(tmp, name);
  writeFileSync(stubPath, source, "utf8");
  chmodSync(stubPath, 0o755);
  return stubPath;
}

/** Stub that writes `reportJson` to the `--report <path>` argv slot, if present. */
function stubSource(reportJson: string, exitCode: number): string {
  return [
    "#!/usr/bin/env node",
    '"use strict";',
    'const fs = require("node:fs");',
    "const args = process.argv.slice(2);",
    'const i = args.indexOf("--report");',
    'if (i !== -1 && typeof args[i + 1] === "string") {',
    "  try { fs.writeFileSync(args[i + 1], " + JSON.stringify(reportJson) + "); } catch {}",
    "}",
    "process.exit(" + exitCode + ");",
    "",
  ].join("\n");
}

const stubPass = writeExecutableStub("stub-pass.js", stubSource(PASS_REPORT, 0));
const stubFail = writeExecutableStub("stub-fail.js", stubSource(FAIL_REPORT, 1));
const stubUsage = writeExecutableStub(
  "stub-usage.js",
  [
    "#!/usr/bin/env node",
    '"use strict";',
    'process.stderr.write("boom: C-01\\n");',
    "process.exit(2);",
    "",
  ].join("\n"),
);

// The stubs never read the config; it only has to be a plausible path.
const configPath = join(tmp, "stub.test.json");
writeFileSync(
  configPath,
  JSON.stringify({
    version: 1,
    name: "stub",
    world: "living_room",
    trajectory: "t.json",
  }),
  "utf8",
);

// Compiled CLI wrapper (dist/src/cli.js), reached from dist/test/.
const cliJs = fileURLToPath(new URL("../src/cli.js", import.meta.url));

after(() => {
  rmSync(tmp, { recursive: true, force: true });
});

test("runTest returns exitCode 0 and the parsed report for a passing run", () => {
  const reportPath = join(tmp, "pass-report.json");
  const result = runTest(configPath, { bin: stubPass, reportPath });
  assert.equal(result.exitCode, 0);
  assert.ok(result.report);
  assert.equal(result.report?.summary.total, 1);
});

test("runTest returns exitCode 1 and the parsed report for a failing run", () => {
  const reportPath = join(tmp, "fail-report.json");
  const result = runTest(configPath, { bin: stubFail, reportPath });
  assert.equal(result.exitCode, 1);
  assert.equal(result.report?.summary.fail, 1);
});

test("runTest throws with the CLI's stderr when the binary exits 2", () => {
  assert.throws(
    () => runTest(configPath, { bin: stubUsage }),
    (err: unknown) =>
      err instanceof Error && err.message === "lookwright failed: boom: C-01",
  );
});

test("runTest throws the install hint when opts.bin does not exist", () => {
  assert.throws(
    () => runTest(configPath, { bin: join(tmp, "no-such-binary") }),
    (err: unknown) =>
      err instanceof Error &&
      err.message ===
        "lookwright binary not found. Install the Rust CLI (cargo install --git https://github.com/daikiito-dk/Lookwright lookwright-cli) or set LOOKWRIGHT_BIN.",
  );
});

test("runTest resolves the binary from $LOOKWRIGHT_BIN when opts.bin is unset", () => {
  const previous = process.env.LOOKWRIGHT_BIN;
  process.env.LOOKWRIGHT_BIN = stubPass;
  try {
    const reportPath = join(tmp, "env-report.json");
    const result = runTest(configPath, { reportPath });
    assert.equal(result.exitCode, 0);
    assert.equal(result.report?.summary.total, 1);
  } finally {
    if (previous === undefined) {
      delete process.env.LOOKWRIGHT_BIN;
    } else {
      process.env.LOOKWRIGHT_BIN = previous;
    }
  }
});

test("runTest returns report: null when no reportPath is given", () => {
  const result = runTest(configPath, { bin: stubPass });
  assert.equal(result.exitCode, 0);
  assert.equal(result.report, null);
});

test("runTest returns {exitCode: 1, report: null} when the report file is not valid JSON", () => {
  const reportPath = join(tmp, "garbage-report.json");
  writeFileSync(reportPath, "{ this is not valid json", "utf8");
  // Write-protect the file so the stub cannot overwrite the garbage; the
  // runner must swallow the parse failure and return null instead of throwing.
  chmodSync(reportPath, 0o444);
  try {
    const result = runTest(configPath, { bin: stubFail, reportPath });
    assert.equal(result.exitCode, 1);
    assert.equal(result.report, null);
  } finally {
    chmodSync(reportPath, 0o644);
  }
});

test("CLI wrapper forwards args to the native binary and propagates its exit code", () => {
  const reportPath = join(tmp, "wrapper-report.json");
  const env = { ...process.env, LOOKWRIGHT_BIN: stubPass };
  const proc = spawnSync(
    process.execPath,
    [cliJs, "test", "--config", configPath, "--report", reportPath],
    { encoding: "utf8", env },
  );
  assert.equal(proc.status, 0);
  const report = JSON.parse(readFileSync(reportPath, "utf8"));
  assert.equal(report.summary.total, 1);
});

test("CLI wrapper exits 2 with the install hint when the binary is missing", () => {
  // A PATH that contains no `lookwright` and no LOOKWRIGHT_BIN override.
  const emptyPathDir = join(tmp, "empty-bin-dir");
  mkdirSync(emptyPathDir);
  const env: NodeJS.ProcessEnv = { ...process.env, PATH: emptyPathDir };
  delete env.LOOKWRIGHT_BIN;
  const proc = spawnSync(
    process.execPath,
    [cliJs, "test", "--config", configPath],
    { encoding: "utf8", env },
  );
  assert.equal(proc.status, 2);
  assert.match(proc.stderr ?? "", /lookwright binary not found on PATH\./);
  assert.match(
    proc.stderr ?? "",
    /cargo install --git https:\/\/github\.com\/daikiito-dk\/Lookwright lookwright-cli/,
  );
});

// Under `npx`, node_modules/.bin (holding this wrapper as `lookwright`) comes
// first on PATH. The wrapper must skip itself and find the native binary.
const skipPosix = process.platform === "win32";

/** A directory whose `lookwright` is a symlink to the wrapper, as npm installs it. */
function wrapperBinDir(): string {
  const dir = join(tmp, "node-modules-bin");
  mkdirSync(dir, { recursive: true });
  try {
    symlinkSync(cliJs, join(dir, "lookwright"));
  } catch {
    // already created by an earlier test
  }
  chmodSync(cliJs, 0o755);
  return dir;
}

/** A directory whose `lookwright` is a non-Node executable standing in for the native CLI. */
function nativeBinDir(): string {
  const dir = join(tmp, "native-bin");
  mkdirSync(dir, { recursive: true });
  const bin = join(dir, "lookwright");
  writeFileSync(bin, "#!/bin/sh\necho native-lookwright \"$@\"\nexit 0\n", "utf8");
  chmodSync(bin, 0o755);
  return dir;
}

test("findNativeOnPath skips the Node wrapper and returns the native binary behind it", { skip: skipPosix }, () => {
  const pathEnv = [wrapperBinDir(), nativeBinDir()].join(delimiter);
  assert.equal(findNativeOnPath(pathEnv), join(tmp, "native-bin", "lookwright"));
});

test("CLI wrapper does not spawn itself when its own bin dir comes first on PATH", { skip: skipPosix }, () => {
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    PATH: [wrapperBinDir(), nativeBinDir(), process.env.PATH ?? ""].join(delimiter),
  };
  delete env.LOOKWRIGHT_BIN;
  const proc = spawnSync(process.execPath, [cliJs, "--version"], {
    encoding: "utf8",
    env,
    timeout: 10_000,
  });
  assert.equal(proc.status, 0, proc.stderr);
  assert.match(proc.stdout, /native-lookwright --version/);
});

test("CLI wrapper exits 2 when the only lookwright on PATH is the wrapper itself", { skip: skipPosix }, () => {
  const env: NodeJS.ProcessEnv = { ...process.env, PATH: wrapperBinDir() };
  delete env.LOOKWRIGHT_BIN;
  const proc = spawnSync(process.execPath, [cliJs, "--version"], {
    encoding: "utf8",
    env,
    timeout: 10_000,
  });
  assert.equal(proc.status, 2);
  assert.match(proc.stderr ?? "", /lookwright binary not found on PATH\./);
});

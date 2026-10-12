import { spawnSync } from "node:child_process";
import { accessSync, closeSync, constants, openSync, readFileSync, readSync } from "node:fs";
import { delimiter, join } from "node:path";
import type { Report } from "./types.js";

/** Options for {@link runTest}. */
export interface RunOptions {
  /** Explicit path to (or name of) the native `lookwright` binary. */
  bin?: string;
  /** Write the report JSON (SPEC §11) here via `--report`. */
  reportPath?: string;
  /** Write the JUnit XML here via `--junit`. */
  junitPath?: string;
  /** Working directory for the spawned binary. */
  cwd?: string;
}

/** Result of a {@link runTest} call. */
export interface RunResult {
  /** `0` all assertions passed, `1` at least one failed (SPEC §10). */
  exitCode: 0 | 1;
  /**
   * Parsed `--report` JSON, or `null` when `reportPath` was not given or the
   * report file is missing/unparsable.
   */
  report: Report | null;
}

const BINARY_NOT_FOUND =
  "lookwright binary not found. Install the Rust CLI (cargo install --git https://github.com/daikiito-dk/Lookwright lookwright-cli) or set LOOKWRIGHT_BIN.";

/**
 * Binary resolution order (SPEC §14): explicit override → `$LOOKWRIGHT_BIN` →
 * `lookwright` on PATH. No silent downloads in v0.1.
 *
 * The PATH lookup skips Node scripts: under `npx`, `node_modules/.bin` comes
 * first on PATH and its `lookwright` is this wrapper, so spawning the bare name
 * would make the wrapper spawn itself forever. Returns `undefined` when no
 * native binary is found.
 */
export function resolveLookwrightBin(explicit?: string): string | undefined {
  if (explicit !== undefined) return explicit;
  if (process.env.LOOKWRIGHT_BIN !== undefined) return process.env.LOOKWRIGHT_BIN;
  return process.platform === "win32" ? "lookwright" : findNativeOnPath();
}

/**
 * First `lookwright` on PATH that is not a Node script. On Windows, spawn
 * without a shell only runs `.exe`, so npm's `.cmd` shim cannot be picked up
 * and the plain name is left to the OS.
 */
export function findNativeOnPath(pathEnv: string = process.env.PATH ?? ""): string | undefined {
  if (process.platform === "win32") return undefined;
  for (const dir of pathEnv.split(delimiter)) {
    if (!dir) continue;
    const candidate = join(dir, "lookwright");
    try {
      accessSync(candidate, constants.X_OK);
    } catch {
      continue;
    }
    if (!isNodeScript(candidate)) return candidate;
  }
  return undefined;
}

function isNodeScript(file: string): boolean {
  let fd: number | undefined;
  try {
    fd = openSync(file, "r");
    const head = Buffer.alloc(128);
    const n = readSync(fd, head, 0, head.length, 0);
    const firstLine = head.subarray(0, n).toString("utf8").split("\n")[0];
    return firstLine.startsWith("#!") && firstLine.includes("node");
  } catch {
    return false;
  } finally {
    if (fd !== undefined) closeSync(fd);
  }
}

function parseReportFile(reportPath: string): Report | null {
  let text: string;
  try {
    text = readFileSync(reportPath, "utf8");
  } catch {
    return null;
  }
  try {
    const parsed: unknown = JSON.parse(text);
    return typeof parsed === "object" && parsed !== null ? (parsed as Report) : null;
  } catch {
    return null;
  }
}

/**
 * Run `lookwright test --config <configPath>` (SPEC §10) and return the exit
 * code plus the parsed report.
 *
 * - Exit codes `0`/`1` are returned as `{ exitCode, report }`.
 * - Exit code `2` (usage/validation error) and crashes (signal, `status === null`)
 *   throw `Error("lookwright failed: <stderr>")`.
 * - A missing/unspawnable binary throws an install-hint error.
 * - `report` is parsed from `opts.reportPath` when that option was given and the
 *   file exists and parses; any read/parse failure yields `null` (never throws).
 */
export function runTest(configPath: string, opts: RunOptions = {}): RunResult {
  const args: string[] = ["test", "--config", configPath];
  if (opts.reportPath !== undefined) args.push("--report", opts.reportPath);
  if (opts.junitPath !== undefined) args.push("--junit", opts.junitPath);

  const bin = resolveLookwrightBin(opts.bin);
  if (bin === undefined) {
    throw new Error(BINARY_NOT_FOUND);
  }

  const proc = spawnSync(bin, args, {
    encoding: "utf8",
    cwd: opts.cwd,
  });

  if (proc.error) {
    throw new Error(BINARY_NOT_FOUND);
  }

  const status = proc.status;
  if (status === 0 || status === 1) {
    const report =
      opts.reportPath !== undefined ? parseReportFile(opts.reportPath) : null;
    return { exitCode: status, report };
  }

  // Exit code 2 (usage/validation error), a signal/crash (status === null),
  // or any non-contract exit code: surface the CLI's stderr.
  throw new Error("lookwright failed: " + (proc.stderr ?? "").trim());
}

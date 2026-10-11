import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
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
 */
export function resolveLookwrightBin(explicit?: string): string {
  return explicit ?? process.env.LOOKWRIGHT_BIN ?? "lookwright";
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

  const proc = spawnSync(resolveLookwrightBin(opts.bin), args, {
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

// Report JSON types, mirroring SPEC.md §11 field-for-field (same names,
// snake_case). The CLI writes this shape via `--report`.

export interface AssertionResult {
  code: string; // "A-01" .. "A-05" (SPEC §9)
  type: string; // "ui_visible" | "gaze_focused" | "anchor_drift" | "event_fired" | "golden_visual"
  pass: boolean;
  detail: string;
  metrics: Record<string, number>;
}

export interface Report {
  schema: "lookwright/report@1";
  name: string;
  target: string | null;
  world: string;
  trajectory: string;
  samples: number;
  assertions: AssertionResult[];
  summary: { pass: number; fail: number; total: number; exit_code: number };
}

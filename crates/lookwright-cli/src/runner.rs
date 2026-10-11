//! Test runner: sampling loop, assertion evaluation, reports
//! (SPEC.md §6, §9, §11; exit codes §10).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use lookwright_core::error::Issue;
use lookwright_core::{CameraParams, Projector, Trajectory, UiPlacement, Vec3};
use lookwright_perception::{cast_gaze, World};
use lookwright_renderer as renderer;
use lookwright_renderer::{Frame, MeshBatch};

use crate::assert::AssertionSpec;
use crate::config::{TestConfig, VALID_TARGETS};
use crate::junit;
use crate::report::{AssertionReport, Report, Summary, REPORT_SCHEMA};
use crate::RunArgs;

/// `lookwright run` / `lookwright test` entry point.
pub fn run(args: &RunArgs) -> Result<i32, Issue> {
    // `--headless` is accepted for CI muscle memory; Lookwright is always
    // headless (SPEC.md §1/§10).
    let _ = args.headless;
    let cfg = TestConfig::load(&args.config)?;
    let target = match (&args.target, &cfg.target) {
        (Some(t), _) => Some(t.clone()),
        (None, c) => c.clone(),
    };
    if let Some(t) = &target {
        if !VALID_TARGETS.contains(&t.as_str()) {
            return Err(Issue::new(
                "C-01",
                "/target",
                format!("unknown target {t:?}; expected one of {VALID_TARGETS:?}"),
            ));
        }
    }
    run_config(
        &cfg,
        target.as_deref(),
        args.report.as_deref(),
        args.junit.as_deref(),
    )
}

/// Execute a validated config end to end and emit reports.
pub fn run_config(
    cfg: &TestConfig,
    target: Option<&str>,
    report_path: Option<&Path>,
    junit_path: Option<&Path>,
) -> Result<i32, Issue> {
    let world = cfg.load_world()?;
    let traj = cfg.load_trajectory()?;

    // C-02: gaze targets must exist in the loaded world (SPEC.md §7).
    let world_ids: BTreeSet<String> = world.objects().iter().map(|o| o.id().to_string()).collect();
    for (i, a) in cfg.assertions.iter().enumerate() {
        if let Some(g) = a.gaze_ref() {
            if !world_ids.contains(g) {
                return Err(Issue::new(
                    "C-02",
                    format!("/assertions/{i}/target"),
                    format!("assertion references unknown world object {g:?}"),
                ));
            }
        }
    }

    let params = cfg.camera_params();
    let dt = 1.0 / cfg.sample_hz;
    let duration = traj.duration_sec();
    // Tolerant floor: 8.0 s at 30 hz must include the t = 8.0 sample (241
    // samples) even if the float ratio lands one ULP below the integer
    // (SPEC.md §6: samples run at t = 0, dt, 2·dt, … ≤ duration).
    let steps = (duration * cfg.sample_hz + 1e-3).floor() as u32;

    // ui id -> index
    let ui_index: BTreeMap<&str, usize> = cfg
        .ui
        .iter()
        .enumerate()
        .map(|(i, u)| (u.spec.id.as_str(), i))
        .collect();

    // Per-assertion / per-UI accumulators.
    struct A01State {
        visible_sec: f32,
        coverage_sum: f32,
        coverage_n: u32,
    }
    let mut a01: Vec<A01State> = cfg
        .assertions
        .iter()
        .map(|_| A01State {
            visible_sec: 0.0,
            coverage_sum: 0.0,
            coverage_n: 0,
        })
        .collect();
    let mut gaze_sec: BTreeMap<String, f32> = BTreeMap::new();
    let mut ui_drift: Vec<f32> = vec![0.0; cfg.ui.len()];
    let mut fired: Vec<&lookwright_core::Event> = Vec::new();

    for i in 0..=steps {
        let t = (i as f32 * dt).min(duration);
        let sample = traj.sample(t);
        let projector = Projector::new(&sample.camera, &params, cfg.resolution_px);

        // Gaze focus accumulation (A-02).
        if let Some(hit) = cast_gaze(&world, sample.camera.position_m, sample.camera.forward()) {
            *gaze_sec.entry(hit.object_id.clone()).or_insert(0.0) += dt;
        }

        // Events fired in (t-dt, t] (A-04).
        for ev in traj.events_between(t - dt, t) {
            fired.push(ev);
        }

        // Per-UI coverage, occlusion, drift (A-01, A-03).
        let mut coverage: Vec<f32> = Vec::with_capacity(cfg.ui.len());
        let mut occluded: Vec<bool> = Vec::with_capacity(cfg.ui.len());
        for (j, ui) in cfg.ui.iter().enumerate() {
            let placement = ui.spec.place(t, &sample.camera);
            let pts = placement.grid_points(4);
            let mut inside = 0u32;
            for p in &pts {
                if let Some([px, py]) = projector.project(*p) {
                    if px >= 0.0
                        && px <= cfg.resolution_px[0] as f32
                        && py >= 0.0
                        && py <= cfg.resolution_px[1] as f32
                    {
                        inside += 1;
                    }
                }
            }
            coverage.push(inside as f32 / pts.len() as f32);
            let to_center = placement.center_m - sample.camera.position_m;
            let dist = to_center.length();
            let blocked = cast_gaze(&world, sample.camera.position_m, to_center.normalized())
                .map(|h| h.distance_m < dist - 1e-3)
                .unwrap_or(false);
            occluded.push(blocked);
            ui_drift[j] = ui_drift[j].max(placement.drift_m);
        }

        // A-01 accumulation per assertion.
        for (ai, a) in cfg.assertions.iter().enumerate() {
            if let AssertionSpec::UiVisible {
                ui,
                min_coverage,
                require_unoccluded,
                ..
            } = a
            {
                let j = ui_index[ui.as_str()];
                if coverage[j] >= *min_coverage && (!*require_unoccluded || !occluded[j]) {
                    let st = &mut a01[ai];
                    st.visible_sec += dt;
                    st.coverage_sum += coverage[j];
                    st.coverage_n += 1;
                }
            }
        }
    }

    // Evaluate assertions in config order (SPEC.md §9).
    let mut results: Vec<AssertionReport> = Vec::with_capacity(cfg.assertions.len());
    for (ai, a) in cfg.assertions.iter().enumerate() {
        match a {
            AssertionSpec::UiVisible {
                ui, duration_sec, ..
            } => {
                let st = &a01[ai];
                let pass = st.visible_sec >= *duration_sec;
                let mean_cov = if st.coverage_n > 0 {
                    st.coverage_sum / st.coverage_n as f32
                } else {
                    0.0
                };
                let detail = if pass {
                    format!(
                        "visible {:.2}s of {:.2}s (required {:.2}s, coverage {:.2})",
                        st.visible_sec, duration, duration_sec, mean_cov
                    )
                } else {
                    format!(
                        "visible only {:.2}s of {:.2}s, required {:.2}s",
                        st.visible_sec, duration, duration_sec
                    )
                };
                let mut metrics = BTreeMap::new();
                metrics.insert("visible_sec".to_string(), st.visible_sec);
                metrics.insert("mean_coverage".to_string(), mean_cov);
                results.push(AssertionReport {
                    code: a.code(),
                    kind: a.kind().to_string(),
                    pass,
                    detail,
                    metrics,
                    subject: Some(ui.clone()),
                });
            }
            AssertionSpec::GazeFocused {
                target,
                duration_sec,
            } => {
                let got = *gaze_sec.get(target).unwrap_or(&0.0);
                let pass = got >= *duration_sec;
                let detail = if pass {
                    format!("gaze on {target} for {got:.2}s (required {duration_sec:.2}s)")
                } else {
                    format!("gaze on {target} for only {got:.2}s, required {duration_sec:.2}s")
                };
                let mut metrics = BTreeMap::new();
                metrics.insert("gaze_sec".to_string(), got);
                results.push(AssertionReport {
                    code: a.code(),
                    kind: a.kind().to_string(),
                    pass,
                    detail,
                    metrics,
                    subject: Some(target.clone()),
                });
            }
            AssertionSpec::AnchorDrift { ui, max_drift_m } => {
                let d = ui_drift[ui_index[ui.as_str()]];
                let pass = d <= *max_drift_m + 1e-6;
                let detail = if pass {
                    format!("max drift {d:.4}m within {max_drift_m:.4}m")
                } else {
                    format!("max drift {d:.4}m exceeds {max_drift_m:.4}m")
                };
                let mut metrics = BTreeMap::new();
                metrics.insert("max_drift_m".to_string(), d);
                results.push(AssertionReport {
                    code: a.code(),
                    kind: a.kind().to_string(),
                    pass,
                    detail,
                    metrics,
                    subject: Some(ui.clone()),
                });
            }
            AssertionSpec::EventFired { event, name } => {
                let hit = fired.iter().find(|e| {
                    e.kind == *event
                        && name
                            .as_ref()
                            .is_none_or(|n| e.name.as_deref() == Some(n.as_str()))
                });
                let (pass, detail) = match hit {
                    Some(e) => (true, format!("event {event} fired at t={:.3}s", e.t_sec)),
                    None => (
                        false,
                        format!("no matching {event} event fired during the run"),
                    ),
                };
                results.push(AssertionReport {
                    code: a.code(),
                    kind: a.kind().to_string(),
                    pass,
                    detail,
                    metrics: BTreeMap::new(),
                    subject: Some(name.clone().unwrap_or_else(|| event.clone())),
                });
            }
            AssertionSpec::GoldenVisual {
                dir,
                times_sec,
                max_mean_abs_diff,
                max_changed_pixel_ratio,
            } => {
                let (pass, detail, metrics) = eval_golden(
                    cfg,
                    &world,
                    &traj,
                    &params,
                    dir,
                    times_sec,
                    *max_mean_abs_diff,
                    *max_changed_pixel_ratio,
                );
                results.push(AssertionReport {
                    code: a.code(),
                    kind: a.kind().to_string(),
                    pass,
                    detail,
                    metrics,
                    subject: None,
                });
            }
        }
    }

    // Report (SPEC.md §11).
    let pass_n = results.iter().filter(|r| r.pass).count() as u32;
    let fail_n = results.len() as u32 - pass_n;
    let exit_code = i32::from(fail_n > 0);
    let report = Report {
        schema: REPORT_SCHEMA,
        name: cfg.name.clone(),
        target: target.map(str::to_string),
        world: cfg.world_ref.clone(),
        trajectory: cfg.trajectory_ref.clone(),
        samples: steps + 1,
        assertions: results.clone(),
        summary: Summary {
            pass: pass_n,
            fail: fail_n,
            total: results.len() as u32,
            exit_code,
        },
    };

    if let Some(p) = report_path {
        let mut json = serde_json::to_string_pretty(&report)
            .map_err(|e| Issue::new("P-01", p.display().to_string(), format!("{e}")))?;
        json.push('\n');
        std::fs::write(p, json).map_err(|e| {
            Issue::new(
                "P-01",
                p.display().to_string(),
                format!("cannot write report: {e}"),
            )
        })?;
    }
    if let Some(p) = junit_path {
        junit::write_junit(p, &cfg.name, &report.assertions).map_err(|e| {
            Issue::new(
                "P-01",
                p.display().to_string(),
                format!("cannot write junit: {e}"),
            )
        })?;
    }

    // Human table (stdout; diagnostics go to stderr — SPEC.md §10).
    println!("lookwright: {}", cfg.name);
    for r in &results {
        let label = match &r.subject {
            Some(s) => format!("{} {s}", r.kind),
            None => r.kind.clone(),
        };
        println!("{}  {}  {}: {}", r.code, pass_fail(r.pass), label, r.detail);
    }
    println!("summary: {pass_n} pass, {fail_n} fail");
    Ok(exit_code)
}

fn pass_fail(pass: bool) -> &'static str {
    if pass {
        "PASS"
    } else {
        "FAIL"
    }
}

/// Compare rendered frames against golden images (A-05, SPEC.md §9/§12).
#[allow(clippy::too_many_arguments)]
fn eval_golden(
    cfg: &TestConfig,
    world: &World,
    traj: &Trajectory,
    params: &CameraParams,
    dir: &Path,
    times_sec: &[f32],
    max_mean_abs_diff: f32,
    max_changed_pixel_ratio: f32,
) -> (bool, String, BTreeMap<String, f32>) {
    let mut pass = true;
    let mut first_msg = String::new();
    let mut worst_mean = 0.0f32;
    let mut worst_ratio = 0.0f32;
    let mut missing = 0u32;
    let frames = times_sec.len() as u32;

    for &t in times_sec {
        let path = golden_path(dir, t);
        let golden = match renderer::read_rgb_png(&path) {
            Ok(g) => g,
            Err(_) => {
                missing += 1;
                pass = false;
                if first_msg.is_empty() {
                    first_msg = format!(
                        "missing golden frame {} (run `lookwright golden update --config <config>`)",
                        path.display()
                    );
                }
                continue;
            }
        };
        let frame = render_scene(cfg, world, traj, params, t);
        match renderer::diff(&golden, &frame) {
            Ok(m) => {
                worst_mean = worst_mean.max(m.mean_abs_diff);
                worst_ratio = worst_ratio.max(m.changed_pixel_ratio);
                if m.mean_abs_diff > max_mean_abs_diff
                    || m.changed_pixel_ratio > max_changed_pixel_ratio
                {
                    pass = false;
                    if first_msg.is_empty() {
                        first_msg = format!(
                            "frame {} differs: mean_abs_diff {:.3} (max {:.3}), changed_pixel_ratio {:.5} (max {:.5})",
                            path.display(),
                            m.mean_abs_diff,
                            max_mean_abs_diff,
                            m.changed_pixel_ratio,
                            max_changed_pixel_ratio
                        );
                    }
                }
            }
            Err(e) => {
                pass = false;
                if first_msg.is_empty() {
                    first_msg = format!("cannot compare {}: {e}", path.display());
                }
            }
        }
    }

    let detail = if pass {
        format!(
            "{frames} frame(s) within thresholds (worst mean_abs_diff {worst_mean:.3}, changed_pixel_ratio {worst_ratio:.5})"
        )
    } else {
        first_msg
    };
    let mut metrics = BTreeMap::new();
    metrics.insert("frames".to_string(), frames as f32);
    metrics.insert("missing_frames".to_string(), missing as f32);
    metrics.insert("worst_mean_abs_diff".to_string(), worst_mean);
    metrics.insert("worst_changed_pixel_ratio".to_string(), worst_ratio);
    (pass, detail, metrics)
}

/// Golden image file name: `frame_<t_millis>.png` (SPEC.md §12).
pub(crate) fn golden_path(dir: &Path, t_sec: f32) -> PathBuf {
    dir.join(format!("frame_{}.png", (t_sec * 1000.0).round() as u64))
}

/// Render one frame at `t_sec` (world objects + UI quads, SPEC.md §12).
pub(crate) fn render_scene(
    cfg: &TestConfig,
    world: &World,
    traj: &Trajectory,
    params: &CameraParams,
    t_sec: f32,
) -> Frame {
    let sample = traj.sample(t_sec);
    let batches = scene_batches(cfg, world, traj, t_sec);
    renderer::render(&batches, &sample.camera, params, cfg.resolution_px)
}

/// Mesh batches for a frame: every world object plus every UI quad
/// (SPEC.md §5 `color`, §12).
pub(crate) fn scene_batches(
    cfg: &TestConfig,
    world: &World,
    traj: &Trajectory,
    t_sec: f32,
) -> Vec<MeshBatch> {
    let sample = traj.sample(t_sec);
    let mut batches: Vec<MeshBatch> = world
        .objects()
        .iter()
        .map(|o| MeshBatch {
            triangles_m: o.triangles_m(),
            color: o.color_rgb(),
        })
        .collect();
    for ui in &cfg.ui {
        let placement = ui.spec.place(t_sec, &sample.camera);
        batches.push(MeshBatch {
            triangles_m: quad_triangles(&placement),
            color: ui.color,
        });
    }
    batches
}

fn quad_triangles(p: &UiPlacement) -> Vec<[Vec3; 3]> {
    vec![
        [p.corners_m[0], p.corners_m[1], p.corners_m[2]],
        [p.corners_m[0], p.corners_m[2], p.corners_m[3]],
    ]
}

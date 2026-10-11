//! CLI integration tests (SPEC.md §10): exit codes 0/1/2, reports, goldens,
//! perception feed, worlds, and validation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lookwright"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("repo root")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lookwright-cli-{tag}-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn run_test_config(config: &Path, extra: &[&str]) -> Output {
    let mut cmd = bin();
    cmd.arg("test").arg("--config").arg(config);
    for arg in extra {
        cmd.arg(arg);
    }
    cmd.output().expect("spawn lookwright")
}

#[test]
fn walk_and_look_passes_end_to_end() {
    let cfg = repo_root()
        .join("examples/tests")
        .join("walk_and_look.test.json");
    let out = run_test_config(&cfg, &[]);
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    let so = stdout(&out);
    assert!(so.contains("A-01"));
    assert!(so.contains("PASS"));
    assert!(so.contains("summary: 7 pass, 0 fail"), "got: {so}");
}

#[test]
fn report_json_matches_spec_schema() {
    let tmp = temp_dir("report");
    let report_path = tmp.join("report.json");
    let cfg = repo_root()
        .join("examples/tests")
        .join("walk_and_look.test.json");
    let out = run_test_config(&cfg, &["--report", report_path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    let text = fs::read_to_string(&report_path).expect("report");
    let report: serde_json::Value = serde_json::from_str(&text).expect("report json");
    assert_eq!(report["schema"], "lookwright/report@1");
    assert_eq!(report["name"], "walk-and-look");
    assert_eq!(report["target"], "ray-ban-meta");
    assert_eq!(report["world"], "living_room");
    assert_eq!(report["samples"], 241);
    assert_eq!(report["summary"]["pass"], 7);
    assert_eq!(report["summary"]["fail"], 0);
    assert_eq!(report["summary"]["exit_code"], 0);
    assert_eq!(report["assertions"].as_array().unwrap().len(), 7);
    let a01 = &report["assertions"][0];
    assert_eq!(a01["code"], "A-01");
    assert_eq!(a01["type"], "ui_visible");
    assert_eq!(a01["pass"], true);
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn faulty_anchor_fails_with_exit_1_and_junit_failure() {
    let tmp = temp_dir("faulty");
    let junit_path = tmp.join("junit.xml");
    let cfg = repo_root()
        .join("examples/tests")
        .join("faulty_anchor.test.json");
    let out = run_test_config(&cfg, &["--junit", junit_path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let so = stdout(&out);
    assert!(so.contains("FAIL"), "got: {so}");
    assert!(so.contains("exceeds"), "got: {so}");
    let xml = fs::read_to_string(&junit_path).expect("junit");
    assert!(xml.contains("<testsuite name=\"lookwright: faulty-anchor\""));
    assert!(xml.contains("<failure"));
    assert!(xml.contains("A-03: anchor_drift wobbly-card"));
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn validate_reports_coded_issues_with_exit_2() {
    let tmp = temp_dir("validate");
    let bad = tmp.join("bad.json");
    fs::write(
        &bad,
        r#"{ "head": [
              { "t_sec": 1.0, "position_m": [0, 1.7, 0] },
              { "t_sec": 1.0, "position_m": [1, 1.7, 0] }
            ] }"#,
    )
    .unwrap();
    let out = bin()
        .arg("validate")
        .arg("--trajectory")
        .arg(&bad)
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    let se = stderr(&out);
    assert!(se.contains("T-02"), "got: {se}");
    assert!(se.contains("/head/1/t_sec"), "got: {se}");

    // A valid example validates cleanly.
    let cfg = repo_root()
        .join("examples/tests")
        .join("walk_and_look.test.json");
    let out = bin()
        .arg("validate")
        .arg("--config")
        .arg(&cfg)
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(stdout(&out).starts_with("OK config"));
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn worlds_list_and_show() {
    let out = bin().arg("worlds").arg("list").output().expect("spawn");
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let so = stdout(&out);
    for id in ["living_room", "office", "urban_street", "warehouse"] {
        assert!(so.contains(id), "missing preset {id} in: {so}");
    }

    let out = bin()
        .arg("worlds")
        .arg("show")
        .arg("living_room")
        .output()
        .expect("spawn");
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let doc: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("world json");
    let objects = doc["objects"].as_array().expect("objects");
    assert_eq!(objects.len(), 10);
    assert!(objects.iter().any(|o| o["id"] == "tv"));

    let out = bin()
        .arg("worlds")
        .arg("show")
        .arg("nope")
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("W-07"));
}

#[test]
fn perception_feed_is_valid_jsonld() {
    let traj = repo_root()
        .join("examples/trajectories")
        .join("walk_and_look_left.json");
    let out = bin()
        .arg("perception")
        .arg("--world")
        .arg("living_room")
        .arg("--trajectory")
        .arg(&traj)
        .arg("--at-sec")
        .arg("1.0")
        .arg("--format")
        .arg("jsonld")
        .output()
        .expect("spawn");
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let doc: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("frame json");
    assert_eq!(doc["@type"], "PerceptionFrame");
    assert_eq!(doc["schema"], "lookwright/perception-frame@1");
    assert_eq!(doc["camera"]["gaze_target"], "tv");
    let objects = doc["visible_objects"].as_array().expect("visible");
    assert!(!objects.is_empty());
    for o in objects {
        assert_eq!(o["@type"], "SpatialObject");
    }

    // Multi-frame dump is a JSON array.
    let out = bin()
        .arg("perception")
        .arg("--world")
        .arg("living_room")
        .arg("--trajectory")
        .arg(&traj)
        .arg("--at-sec")
        .arg("0.5")
        .output()
        .expect("spawn");
    let doc: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("frame json");
    assert_eq!(doc["t_sec"], 0.5);
}

#[test]
fn golden_update_writes_frames() {
    let tmp = temp_dir("golden");
    let traj = repo_root()
        .join("examples/trajectories")
        .join("walk_and_look_left.json");
    let cfg_path = tmp.join("golden-test.test.json");
    let cfg = format!(
        r#"{{
  "version": 1,
  "name": "golden-update",
  "world": "living_room",
  "trajectory": "{}",
  "assertions": [
    {{ "type": "golden_visual", "dir": "gold", "times_sec": [0.5] }}
  ]
}}"#,
        traj.display().to_string().replace('\\', "\\\\")
    );
    fs::write(&cfg_path, cfg).unwrap();
    let out = bin()
        .arg("golden")
        .arg("update")
        .arg("--config")
        .arg(&cfg_path)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    let frame = tmp.join("gold").join("frame_500.png");
    assert!(frame.exists(), "missing {}", frame.display());
    assert!(fs::metadata(&frame).unwrap().len() > 100);

    // Re-running the config now passes its golden assertion (exit 0).
    let out = run_test_config(&cfg_path, &[]);
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&out),
        stderr(&out)
    );
    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn usage_and_validation_errors_exit_2() {
    // Missing config file -> exit 2 with P-01.
    let out = bin()
        .arg("test")
        .arg("--config")
        .arg("/no/such/file.json")
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("P-01"));

    // Unknown subcommand -> clap usage error (exit 2).
    let out = bin().arg("frobnicate").output().expect("spawn");
    assert_eq!(out.status.code(), Some(2));

    // validate with nothing -> exit 2.
    let out = bin().arg("validate").output().expect("spawn");
    assert_eq!(out.status.code(), Some(2));
}

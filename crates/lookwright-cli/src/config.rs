//! Test config parsing and validation (SPEC.md §6; codes C-01…C-06).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use lookwright_core::error::{value_f32, value_vecn, Issue};
use lookwright_core::{CameraParams, Trajectory, UiSpec};
use lookwright_perception::World;
use serde_json::Value;

use crate::assert::AssertionSpec;

/// Device profiles accepted as `target` (SPEC.md §6).
pub const VALID_TARGETS: [&str; 4] = ["ray-ban-meta", "orion", "horizon-os", "generic"];

/// Default UI panel tint when no `color` is set (SPEC.md §5).
pub const DEFAULT_UI_COLOR: [u8; 3] = [0x00, 0xCC, 0xFF];

/// One UI element: the core placement spec plus its render tint (SPEC.md §5).
#[derive(Debug, Clone)]
pub struct UiElement {
    pub spec: UiSpec,
    pub color: [u8; 3],
}

/// A validated test config (SPEC.md §6).
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub name: String,
    pub target: Option<String>,
    /// World reference exactly as written (`living_room` or a path).
    pub world_ref: String,
    /// `Some(path)` when the world is a file; `None` for preset ids.
    pub world_path: Option<PathBuf>,
    /// Trajectory reference exactly as written.
    pub trajectory_ref: String,
    pub trajectory_path: PathBuf,
    pub fov_y_deg: f32,
    pub near_m: f32,
    pub far_m: f32,
    pub resolution_px: [u32; 2],
    pub sample_hz: f32,
    pub ui: Vec<UiElement>,
    pub assertions: Vec<AssertionSpec>,
}

impl TestConfig {
    pub fn load(path: &Path) -> Result<Self, Issue> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            Issue::new(
                "P-01",
                path.display().to_string(),
                format!("cannot read config: {e}"),
            )
        })?;
        let doc: Value = serde_json::from_str(&text).map_err(|e| {
            Issue::new(
                "P-01",
                path.display().to_string(),
                format!("config is not valid JSON: {e}"),
            )
        })?;
        Self::from_value(&doc, path)
    }

    pub fn from_value(doc: &Value, config_path: &Path) -> Result<Self, Issue> {
        let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
        let c01 = |field: &str, msg: String| Issue::new("C-01", format!("/{field}"), msg);

        let name = match doc.get("name") {
            None => config_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".to_string()),
            Some(v) => v
                .as_str()
                .ok_or_else(|| {
                    Issue::new("P-01", "/name", format!("name must be a string, got {v}"))
                })?
                .to_string(),
        };

        let target = match doc.get("target") {
            None => None,
            Some(v) if v.is_null() => None,
            Some(v) => {
                let t = v.as_str().ok_or_else(|| {
                    Issue::new(
                        "P-01",
                        "/target",
                        format!("target must be a string, got {v}"),
                    )
                })?;
                if !VALID_TARGETS.contains(&t) {
                    return Err(c01(
                        "target",
                        format!("unknown target {t:?}; expected one of {VALID_TARGETS:?}"),
                    ));
                }
                Some(t.to_string())
            }
        };

        // world (C-01)
        let world_ref = doc
            .get("world")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                c01(
                    "world",
                    "config requires a world (preset id or path)".into(),
                )
            })?
            .to_string();
        let world_path = if World::preset_ids().contains(&world_ref.as_str()) {
            None
        } else {
            let p = base_dir.join(&world_ref);
            if !p.exists() {
                return Err(c01(
                    "world",
                    format!(
                        "unknown preset and no file at {world_ref:?} (resolved: {})",
                        p.display()
                    ),
                ));
            }
            Some(p)
        };

        // trajectory (C-01)
        let trajectory_ref = doc
            .get("trajectory")
            .and_then(Value::as_str)
            .ok_or_else(|| c01("trajectory", "config requires a trajectory path".into()))?
            .to_string();
        let trajectory_path = base_dir.join(&trajectory_ref);
        if !trajectory_path.exists() {
            return Err(c01(
                "trajectory",
                format!(
                    "no trajectory file at {trajectory_ref:?} (resolved: {})",
                    trajectory_path.display()
                ),
            ));
        }

        // camera (C-03)
        let (fov_y_deg, near_m, far_m) = Self::parse_camera(doc)?;
        // resolution + sampling (C-04)
        let resolution_px = Self::parse_resolution(doc)?;
        let sample_hz = match doc.get("sample_hz") {
            None => 30.0,
            Some(v) if v.is_null() => 30.0,
            Some(v) => {
                let hz = value_f32(v, "/sample_hz", "C-04")?;
                if hz <= 0.0 {
                    return Err(Issue::new(
                        "C-04",
                        "/sample_hz",
                        format!("sample_hz must be > 0, got {hz}"),
                    ));
                }
                hz
            }
        };

        // UI elements (W-06 via UiSpec::from_value; C-02 duplicates)
        let mut ui = Vec::new();
        let mut ui_ids = BTreeSet::new();
        if let Some(arr) = doc.get("ui").and_then(Value::as_array) {
            for (i, v) in arr.iter().enumerate() {
                let path = format!("/ui/{i}");
                let spec = UiSpec::from_value(v, &path)?;
                if !ui_ids.insert(spec.id.clone()) {
                    return Err(Issue::new(
                        "C-02",
                        format!("{path}/id"),
                        format!("duplicate UI id {:?}", spec.id),
                    ));
                }
                let color = match v.get("color") {
                    None => DEFAULT_UI_COLOR,
                    Some(cv) => cv.as_str().and_then(parse_hex_color).ok_or_else(|| {
                        Issue::new(
                            "W-06",
                            format!("{path}/color"),
                            format!("color must be \"#rrggbb\", got {cv}"),
                        )
                    })?,
                };
                ui.push(UiElement { spec, color });
            }
        }
        if let Some(uv) = doc.get("ui") {
            if !uv.is_array() && !uv.is_null() {
                return Err(Issue::new(
                    "P-01",
                    "/ui",
                    format!("ui section must be an array, got {uv}"),
                ));
            }
        }

        // assertions (C-05 / C-06)
        let mut assertions = Vec::new();
        if let Some(arr) = doc.get("assertions").and_then(Value::as_array) {
            for (i, v) in arr.iter().enumerate() {
                assertions.push(AssertionSpec::parse(
                    v,
                    &format!("/assertions/{i}"),
                    base_dir,
                )?);
            }
        }
        if let Some(av) = doc.get("assertions") {
            if !av.is_array() && !av.is_null() {
                return Err(Issue::new(
                    "P-01",
                    "/assertions",
                    format!("assertions section must be an array, got {av}"),
                ));
            }
        }

        // C-02: UI references in assertions must exist.
        for (i, a) in assertions.iter().enumerate() {
            if let Some(ui_ref) = a.ui_ref() {
                if !ui_ids.contains(ui_ref) {
                    return Err(Issue::new(
                        "C-02",
                        format!("/assertions/{i}/{}", a.ui_ref_field().unwrap_or("ui")),
                        format!("assertion references unknown UI id {ui_ref:?}"),
                    ));
                }
            }
        }

        Ok(Self {
            name,
            target,
            world_ref,
            world_path,
            trajectory_ref,
            trajectory_path,
            fov_y_deg,
            near_m,
            far_m,
            resolution_px,
            sample_hz,
            ui,
            assertions,
        })
    }

    fn parse_camera(doc: &Value) -> Result<(f32, f32, f32), Issue> {
        let get = |field: &str, default: f32| -> Result<f32, Issue> {
            match doc.get("camera").and_then(|c| c.get(field)) {
                None => Ok(default),
                Some(v) if v.is_null() => Ok(default),
                Some(v) => value_f32(v, &format!("/camera/{field}"), "C-03"),
            }
        };
        let fov = get("fov_y_deg", 70.0)?;
        let near = get("near_m", 0.05)?;
        let far = get("far_m", 50.0)?;
        if let Some(cv) = doc.get("camera") {
            if !cv.is_object() && !cv.is_null() {
                return Err(Issue::new(
                    "C-03",
                    "/camera",
                    format!("camera must be an object, got {cv}"),
                ));
            }
        }
        if !(1.0..179.0).contains(&fov) {
            return Err(Issue::new(
                "C-03",
                "/camera/fov_y_deg",
                format!("fov_y_deg must be within (1, 179), got {fov}"),
            ));
        }
        if near <= 0.0 {
            return Err(Issue::new(
                "C-03",
                "/camera/near_m",
                format!("near_m must be > 0, got {near}"),
            ));
        }
        if far <= near {
            return Err(Issue::new(
                "C-03",
                "/camera/far_m",
                format!("far_m must be greater than near_m, got far {far} <= near {near}"),
            ));
        }
        Ok((fov, near, far))
    }

    fn parse_resolution(doc: &Value) -> Result<[u32; 2], Issue> {
        let default = json_vec2(640.0, 400.0);
        let raw = match doc.get("resolution_px") {
            None => default,
            Some(v) if v.is_null() => default,
            Some(v) => value_vecn(v, "/resolution_px", "C-04", 2)?,
        };
        let mut out = [0u32; 2];
        for (i, r) in raw.iter().enumerate() {
            if *r < 1.0 || r.fract() != 0.0 || *r > 65535.0 {
                return Err(Issue::new(
                    "C-04",
                    format!("/resolution_px/{i}"),
                    format!("resolution components must be integers in [1, 65535], got {r}"),
                ));
            }
            out[i] = *r as u32;
        }
        Ok(out)
    }

    /// Camera parameters with aspect derived from the resolution (SPEC.md §4).
    pub fn camera_params(&self) -> CameraParams {
        let aspect = self.resolution_px[0] as f32 / self.resolution_px[1] as f32;
        CameraParams::new(self.fov_y_deg, self.near_m, self.far_m, aspect)
    }

    /// Resolve and load the referenced world (preset or file).
    pub fn load_world(&self) -> Result<World, Issue> {
        match &self.world_path {
            Some(p) => World::load(p),
            None => World::from_preset(&self.world_ref),
        }
    }

    /// Load and validate the referenced trajectory.
    pub fn load_trajectory(&self) -> Result<Trajectory, Issue> {
        let text = std::fs::read_to_string(&self.trajectory_path).map_err(|e| {
            Issue::new(
                "P-01",
                self.trajectory_path.display().to_string(),
                format!("cannot read trajectory: {e}"),
            )
        })?;
        Trajectory::parse(&text)
    }
}

fn json_vec2(a: f32, b: f32) -> Vec<f32> {
    vec![a, b]
}

fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn load(v: Value) -> Result<TestConfig, Issue> {
        // The fake config lives in the same temp dir as the fixture
        // trajectory so relative resolution works on every platform
        // (temp_dir() is not /tmp on macOS).
        let cfg = std::env::temp_dir().join("lookwright-test.test.json");
        TestConfig::from_value(&v, &cfg)
    }

    /// Write a minimal valid trajectory into the temp dir so configs that use
    /// `lookwright-test-traj.json` resolve next to the fake config path.
    fn write_temp_trajectory() {
        let dir = std::env::temp_dir();
        std::fs::write(
            dir.join("lookwright-test-traj.json"),
            r#"{ "head": [ { "t_sec": 0.0, "position_m": [0.0, 1.7, 0.0] } ] }"#,
        )
        .expect("temp trajectory");
    }

    fn minimal() -> Value {
        write_temp_trajectory();
        json!({
            "version": 1,
            "name": "t",
            "world": "living_room",
            "trajectory": "lookwright-test-traj.json",
            "ui": [],
            "assertions": []
        })
    }

    #[test]
    fn c01_missing_world_and_trajectory() {
        let e = load(json!({"name": "t"})).unwrap_err();
        assert_eq!(e.code, "C-01");
        assert_eq!(e.path, "/world");

        let e = load(json!({"name": "t", "world": "living_room"})).unwrap_err();
        assert_eq!(e.code, "C-01");
        assert_eq!(e.path, "/trajectory");
    }

    #[test]
    fn c01_unknown_target_and_missing_world_file() {
        let e =
            load(json!({"world": "living_room", "trajectory": "x.json", "target": "vision-pro"}))
                .unwrap_err();
        assert_eq!(e.code, "C-01");
        assert_eq!(e.path, "/target");

        let e = load(json!({"world": "no-such-preset", "trajectory": "x.json"})).unwrap_err();
        assert_eq!(e.code, "C-01");
        assert!(e.message.contains("no file"));
    }

    #[test]
    fn c03_bad_camera() {
        let mut v = minimal();
        v["camera"] = json!({ "fov_y_deg": 200.0 });
        let e = load(v).unwrap_err();
        assert_eq!(e.code, "C-03");
        assert_eq!(e.path, "/camera/fov_y_deg");

        let mut v = minimal();
        v["camera"] = json!({ "near_m": 0.5, "far_m": 0.2 });
        let e = load(v).unwrap_err();
        assert_eq!(e.code, "C-03");
        assert_eq!(e.path, "/camera/far_m");
    }

    #[test]
    fn c04_bad_resolution_and_sample_hz() {
        let mut v = minimal();
        v["resolution_px"] = json!([0, 100]);
        let e = load(v).unwrap_err();
        assert_eq!(e.code, "C-04");

        let mut v = minimal();
        v["sample_hz"] = json!(0.0);
        let e = load(v).unwrap_err();
        assert_eq!(e.code, "C-04");
        assert_eq!(e.path, "/sample_hz");
    }

    #[test]
    fn defaults_are_applied() {
        let cfg = load(minimal()).unwrap();
        assert_eq!(cfg.fov_y_deg, 70.0);
        assert_eq!(cfg.resolution_px, [640, 400]);
        assert_eq!(cfg.sample_hz, 30.0);
        let params = cfg.camera_params();
        assert!((params.aspect - 1.6).abs() < 1e-6);
    }

    #[test]
    fn ui_color_and_w06() {
        let mut v = minimal();
        v["ui"] = json!([
            { "id": "card", "size_m": [0.4, 0.25],
              "anchor": { "mode": "world", "position_m": [0, 1.5, -3] },
              "color": "#ff9900" }
        ]);
        let cfg = load(v).unwrap();
        assert_eq!(cfg.ui[0].color, [0xFF, 0x99, 0x00]);

        let mut v = minimal();
        v["ui"] = json!([
            { "id": "card", "size_m": [0.4, 0.25],
              "anchor": { "mode": "world", "position_m": [0, 1.5, -3] },
              "color": "orange" }
        ]);
        let e = load(v).unwrap_err();
        assert_eq!(e.code, "W-06");
        assert_eq!(e.path, "/ui/0/color");
    }

    #[test]
    fn c02_duplicate_ui_and_dangling_reference() {
        let mut v = minimal();
        v["ui"] = json!([
            { "id": "card", "size_m": [0.1, 0.1], "anchor": { "mode": "world", "position_m": [0, 1, -2] } },
            { "id": "card", "size_m": [0.1, 0.1], "anchor": { "mode": "world", "position_m": [0, 1, -3] } }
        ]);
        let e = load(v).unwrap_err();
        assert_eq!(e.code, "C-02");
        assert_eq!(e.path, "/ui/1/id");

        let mut v = minimal();
        v["assertions"] = json!([
            { "type": "ui_visible", "ui": "ghost" }
        ]);
        let e = load(v).unwrap_err();
        assert_eq!(e.code, "C-02");
        assert!(e.path.starts_with("/assertions/0/ui"));
    }

    #[test]
    fn hex_colors_parse() {
        assert_eq!(parse_hex_color("#00ccff"), Some([0x00, 0xCC, 0xFF]));
        assert_eq!(parse_hex_color("00ccff"), None);
        assert_eq!(parse_hex_color("#00cc"), None);
    }
}

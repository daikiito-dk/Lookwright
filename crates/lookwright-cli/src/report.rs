//! Report JSON structs (SPEC.md §11). Field order is the struct order;
//! metrics use BTreeMap so keys are sorted (determinism, SPEC.md §1).

use std::collections::BTreeMap;

use serde::Serialize;

pub const REPORT_SCHEMA: &str = "lookwright/report@1";

#[derive(Serialize, Debug, Clone)]
pub struct Report {
    pub schema: &'static str,
    pub name: String,
    pub target: Option<String>,
    pub world: String,
    pub trajectory: String,
    pub samples: u32,
    pub assertions: Vec<AssertionReport>,
    pub summary: Summary,
}

#[derive(Serialize, Debug, Clone)]
pub struct AssertionReport {
    pub code: &'static str,
    #[serde(rename = "type")]
    pub kind: String,
    pub pass: bool,
    pub detail: String,
    pub metrics: BTreeMap<String, f32>,
    /// Referenced subject (ui/target id) — used in JUnit testcase names; not
    /// serialized into the report JSON (SPEC.md §11 shows no such field).
    #[serde(skip_serializing)]
    pub subject: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct Summary {
    pub pass: u32,
    pub fail: u32,
    pub total: u32,
    pub exit_code: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> Report {
        let mut metrics = BTreeMap::new();
        metrics.insert("visible_sec".to_string(), 6.9);
        metrics.insert("mean_coverage".to_string(), 0.94);
        Report {
            schema: REPORT_SCHEMA,
            name: "walk-and-look".to_string(),
            target: Some("ray-ban-meta".to_string()),
            world: "living_room".to_string(),
            trajectory: "traj.json".to_string(),
            samples: 240,
            assertions: vec![AssertionReport {
                code: "A-01",
                kind: "ui_visible".to_string(),
                pass: true,
                detail: "visible 6.90s of 8.00s (required 2.00s, coverage 0.94)".to_string(),
                metrics,
                subject: Some("info-card".to_string()),
            }],
            summary: Summary {
                pass: 4,
                fail: 0,
                total: 4,
                exit_code: 0,
            },
        }
    }

    #[test]
    fn serializes_to_spec_shape() {
        let json = serde_json::to_string(&sample_report()).unwrap();
        assert!(json.contains(r#""schema":"lookwright/report@1""#));
        assert!(json.contains(r#""type":"ui_visible""#));
        // metrics keys are sorted
        let idx_vis = json.find("visible_sec").unwrap();
        let idx_cov = json.find("mean_coverage").unwrap();
        assert!(idx_cov < idx_vis, "metrics must be key-sorted: {json}");
        // subject is not part of the report JSON
        assert!(!json.contains("subject"));
        assert!(json.contains(r#""summary":{"pass":4,"fail":0,"total":4,"exit_code":0}"#));
    }
}

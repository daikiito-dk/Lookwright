//! JUnit XML report (SPEC.md §11) — byte-deterministic, no timestamps.

use std::io;
use std::path::Path;

use crate::report::AssertionReport;

/// Write one testsuite covering every assertion result (SPEC.md §11).
pub fn write_junit(path: &Path, suite_name: &str, results: &[AssertionReport]) -> io::Result<()> {
    let total = results.len();
    let failed = results.iter().filter(|r| !r.pass).count();
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<testsuites>\n");
    xml.push_str(&format!(
        "  <testsuite name=\"{}\" tests=\"{total}\" failures=\"{failed}\">\n",
        escape(&format!("lookwright: {suite_name}"))
    ));
    for r in results {
        let mut name = format!("{}: {}", r.code, r.kind);
        if let Some(s) = &r.subject {
            name.push(' ');
            name.push_str(s);
        }
        if r.pass {
            xml.push_str(&format!(
                "    <testcase name=\"{}\" classname=\"lookwright\" />\n",
                escape(&name)
            ));
        } else {
            xml.push_str(&format!(
                "    <testcase name=\"{}\" classname=\"lookwright\">\n",
                escape(&name)
            ));
            xml.push_str(&format!(
                "      <failure message=\"{}\" />\n",
                escape(&r.detail)
            ));
            xml.push_str("    </testcase>\n");
        }
    }
    xml.push_str("  </testsuite>\n</testsuites>\n");
    std::fs::write(path, xml)
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn result(pass: bool, detail: &str) -> AssertionReport {
        AssertionReport {
            code: "A-03",
            kind: "anchor_drift".to_string(),
            pass,
            detail: detail.to_string(),
            metrics: BTreeMap::new(),
            subject: Some("bad-card".to_string()),
        }
    }

    #[test]
    fn writes_pass_fail_and_escapes() {
        let results = vec![
            result(true, "max drift 0.0000m within 0.0100m"),
            result(false, "max drift <b>0.09</b> exceeds 0.01 \"badly\" & more"),
        ];
        let tmp = std::env::temp_dir().join("lookwright-junit-test.xml");
        write_junit(&tmp, "walk & look", &results).unwrap();
        let xml = std::fs::read_to_string(&tmp).unwrap();
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("name=\"lookwright: walk &amp; look\""));
        assert!(xml.contains("tests=\"2\" failures=\"1\""));
        assert!(xml.contains("name=\"A-03: anchor_drift bad-card\""));
        assert!(xml.contains("<failure message=\"max drift &lt;b&gt;0.09&lt;/b&gt; exceeds 0.01 &quot;badly&quot; &amp; more\" />"));
        assert!(!xml.contains("<b>"));
        std::fs::remove_file(&tmp).ok();
    }
}

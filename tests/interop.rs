//! Cross-checks: the sample document in tests/fixtures/sample.md is
//! also rendered by mainstream tools (marked, locally) — the test
//! below pins our own structure for it so regressions show up even
//! without the external tool in CI.

const SAMPLE: &str = include_str!("fixtures/sample.md");

#[test]
fn sample_document_structure() {
    let html = docconvert::convert::convert(SAMPLE, "markdown", "html").unwrap();
    for needle in [
        "<h1>Shopping List</h1>",
        "<strong>buy</strong>",
        "<em>eggs</em>",
        "<li>Bread (fresh)</li>",
        "<h2>Notes</h2>",
        "<blockquote>",
        "<a href=\"https://example.com/coupon\">store coupon</a>",
        "<ol>",
        "<code>inline code</code>",
        "<table>",
        "<th>Item</th>",
        "<td>12</td>",
    ] {
        assert!(html.contains(needle), "missing {needle:?} in:\n{html}");
    }
    // And the HTML converts back to equivalent Markdown.
    let back = docconvert::convert::convert(&html, "html", "markdown").unwrap();
    assert!(back.contains("# Shopping List"), "{back}");
    assert!(back.contains("**buy**"), "{back}");
}

/// Print our HTML for visual diffing against a reference renderer
/// (marked). Enable with DC_DUMP=1.
#[test]
#[ignore = "manual diff aid; DC_DUMP env"]
fn dump_sample_html() {
    if std::env::var("DC_DUMP").is_err() {
        return;
    }
    println!(
        "{}",
        docconvert::convert::convert(SAMPLE, "markdown", "html").unwrap()
    );
}

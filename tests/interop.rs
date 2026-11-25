//! Cross-checks: the sample document in tests/fixtures/sample.md is
//! also rendered by mainstream tools (marked, locally) — the test
//! below pins our own structure for it so regressions show up even
//! without the external tool in CI.

const SAMPLE: &str = include_str!("fixtures/sample.md");
const RELEASE_NOTES: &str = include_str!("fixtures/release-notes.md");

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

/// A document with YAML front matter and reference-style links: the
/// metadata is stripped by default but recoverable, the reference
/// links resolve, and the table survives the HTML round trip.
#[test]
fn front_matter_and_reference_links() {
    let html = docconvert::convert::convert(RELEASE_NOTES, "markdown", "html").unwrap();
    assert!(html.starts_with("<h1>Release 2.0</h1>"), "{html}");
    assert!(!html.contains("title:"), "front matter leaked: {html}");
    assert!(
        html.contains("<a href=\"https://github.com/SeanShaffer88/DocConvert/issues\" title=\"Issue tracker\">tracker</a>"),
        "{html}"
    );
    assert!(
        html.contains(
            "<a href=\"https://example.com/changelog\" title=\"Changelog\">changelog</a>"
        ),
        "{html}"
    );
    assert!(html.contains("<td>Front matter</td>"), "{html}");

    // Opted in, the metadata is rendered instead of dropped.
    let options = docconvert::convert::Options::from_json("{\"front_matter\":\"table\"}").unwrap();
    let kept =
        docconvert::convert::convert_with_options(RELEASE_NOTES, "markdown", "html", &options)
            .unwrap();
    assert!(kept.contains("<td>title</td>"), "{kept}");
    assert!(kept.contains("<td>Release 2.0</td>"), "{kept}");
    // Nested values (the author list) survive as compact JSON.
    assert!(kept.contains("Ada"), "{kept}");

    // Either way the ABI can hand the metadata over as JSON.
    let (front, body) = docconvert::frontmatter::split(RELEASE_NOTES);
    let front = front.expect("front matter");
    assert_eq!(
        front.get("title"),
        Some(&docconvert::json::Json::Str("Release 2.0".into()))
    );
    assert!(body.trim_start().starts_with("# Release 2.0"), "{body}");

    // HTML back to Markdown: reference links arrive inline, and the
    // definitions are gone from the body.
    let back = docconvert::convert::convert(&html, "html", "markdown").unwrap();
    assert!(back.contains("# Release 2.0"), "{back}");
    // Titles survive the trip back out as `dest "title"`.
    assert!(
        back.contains("](https://example.com/changelog \"Changelog\")"),
        "{back}"
    );
    assert!(!back.contains("[changelog]:"), "{back}");
}

/// HTML with a payload table converts to CSV through the same matrix
/// as everything else.
#[test]
fn html_table_to_csv() {
    let html = docconvert::convert::convert(RELEASE_NOTES, "markdown", "html").unwrap();
    let csv = docconvert::convert::convert(&html, "html", "csv").unwrap();
    assert!(
        csv.starts_with("Change,Status\r\n"),
        "csv should start with the table header: {csv}"
    );
    assert!(csv.contains("Reference links,done"), "{csv}");
    assert_eq!(csv.lines().count(), 4, "{csv}");
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

/// The GFM additions survive a full loop: titles reach HTML as
/// `title="…"`, task items become checkboxes, and both come back.
#[test]
fn titles_and_task_lists_round_trip() {
    let md = "See [the docs](https://example.com/docs \"Documentation\").\n\n\
              - [x] shipped\n- [ ] pending\n";
    let html = docconvert::convert::convert(md, "markdown", "html").unwrap();
    assert!(
        html.contains("<a href=\"https://example.com/docs\" title=\"Documentation\">the docs</a>"),
        "{html}"
    );
    assert!(
        html.contains("<li><input type=\"checkbox\" disabled checked> shipped</li>"),
        "{html}"
    );
    assert!(
        html.contains("<li><input type=\"checkbox\" disabled> pending</li>"),
        "{html}"
    );

    let back = docconvert::convert::convert(&html, "html", "markdown").unwrap();
    assert!(
        back.contains("[the docs](https://example.com/docs \"Documentation\")"),
        "{back}"
    );
    assert!(back.contains("- [x] shipped"), "{back}");
    assert!(back.contains("- [ ] pending"), "{back}");
}

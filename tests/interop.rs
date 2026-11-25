//! Cross-checks: the sample document in tests/fixtures/sample.md is
//! also rendered by mainstream tools (marked, locally) — the test
//! below pins our own structure for it so regressions show up even
//! without the external tool in CI.

const SAMPLE: &str = include_str!("fixtures/sample.md");
const RELEASE_NOTES: &str = include_str!("fixtures/release-notes.md");

/// The two new readers reach HTML through the same pipeline as the old
/// ones (Markdown first), and `toc` reads their headings with the same
/// anchors the conversions write.
#[test]
fn docbook_and_mediawiki_reach_html() {
    let book = "<book><title>Guide</title><chapter><title>Install</title>\
                <para>Run <literal>setup</literal>.</para></chapter></book>";
    let html = docconvert::convert::convert(book, "docbook", "html").unwrap();
    for needle in ["<h1>Guide</h1>", "<h2>Install</h2>", "<code>setup</code>"] {
        assert!(html.contains(needle), "missing {needle:?} in:\n{html}");
    }

    let wiki = "== Install ==\n\nRun ''this''.\n\n* one\n* two\n";
    let html = docconvert::convert::convert(wiki, "mediawiki", "html").unwrap();
    for needle in ["<h2>Install</h2>", "<em>this</em>", "<li>one</li>"] {
        assert!(html.contains(needle), "missing {needle:?} in:\n{html}");
    }

    let toc = docconvert::toc::render_from(wiki, "mediawiki", "markdown").unwrap();
    assert!(toc.contains("[Install](#install)"), "{toc}");
    let toc = docconvert::toc::render_from(book, "docbook", "html").unwrap();
    assert!(toc.contains("href=\"#install\""), "{toc}");
}

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

/// Footnotes survive markdown→html→markdown: references come back as
/// `[^n]` and the section as definitions.
#[test]
fn footnotes_round_trip() {
    let md = "Text with a note[^a].\n\n[^a]: The note itself.\n";
    let html = docconvert::convert::convert(md, "markdown", "html").unwrap();
    assert!(
        html.contains("<sup><a href=\"#fn-1\" id=\"fnref-1\">1</a></sup>"),
        "{html}"
    );
    assert!(html.contains("<section class=\"footnotes\">"), "{html}");

    let back = docconvert::convert::convert(&html, "html", "markdown").unwrap();
    assert!(back.contains("Text with a note[^1]."), "{back}");
    assert!(back.contains("[^1]: The note itself."), "{back}");
}

/// TSV travels the whole matrix like CSV does.
#[test]
fn tsv_across_the_matrix() {
    let tsv = "name\tqty\r\nMilk\t2\r\n";
    let json = docconvert::convert::convert(tsv, "tsv", "json").unwrap();
    assert!(json.contains("\"name\": \"Milk\""), "{json}");
    let md = docconvert::convert::convert(tsv, "tsv", "markdown").unwrap();
    assert!(md.starts_with("| name | qty |"), "{md}");
    let html = docconvert::convert::convert(&md, "markdown", "tsv").unwrap();
    assert_eq!(html, tsv);
}

/// The streaming path and the whole-document path write the same bytes for
/// every table pair — the streaming gate redoes the file when it cannot, so
/// a difference here means the gate chose wrong, not just a slower path.
#[test]
fn table_pairs_stream_byte_identically() {
    let dir = std::env::temp_dir().join(format!("docconvert-pairs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let cases: &[(&str, &str, &str)] = &[
        ("name,qty\nMilk,2\nBread,3\n", "csv", "markdown"),
        ("name\tqty\nMilk\t2\n", "tsv", "markdown"),
        (
            "| name | qty |\n| --- | --- |\n| Milk | 2 |\n",
            "markdown",
            "csv",
        ),
        (
            "| name | qty |\n| --- | --- |\n| Milk | 2 |\n",
            "markdown",
            "tsv",
        ),
    ];
    for (index, (source, from, to)) in cases.iter().enumerate() {
        let input = dir.join(format!("case-{index}.{from}"));
        let output = dir.join(format!("case-{index}.out"));
        std::fs::write(&input, source).expect("write input");
        docconvert::convert::convert_file_streaming(
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            Some(from),
            Some(to),
        )
        .unwrap_or_else(|error| panic!("{from} → {to} streamed: {error:?}"));
        let streamed = std::fs::read_to_string(&output).expect("read output");
        let whole = docconvert::convert::convert(source, from, to).expect("whole document");
        assert_eq!(streamed, whole, "{from} → {to} differs when streamed");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

# DocConvert — agent notes

- Zero dependencies by design: json.rs, csv.rs, markdown.rs, html.rs,
  and frontmatter.rs are all hand-written. Verification anchors: the
  unit suites per module, the marked cross-check fixture
  (tests/fixtures/sample.md + tests/interop.rs), the reference-link /
  front-matter fixture (tests/fixtures/release-notes.md), the file API
  tests (tests/file_io.rs), and the matrix test in convert.rs.
- `convert::MATRIX` is the single source of truth for supported
  pairs; `dc_matrix_json()` is generated from it and `matrix_is_exact`
  fails loudly if dispatch and the table disagree (unsupported pairs
  are NOT_SUPPORTED, never silently degraded). Add a pair in one place
  only: `MATRIX` plus the dispatch arm.
- CSV→JSON inference rule is deliberate and documented: numbers,
  booleans, empty→null; leading-zero values stay strings. Don't
  "fix" 007 into 7. Front matter scalars follow the same rule.
- Markdown output safety: raw HTML is escaped on the md→html path.
  Weakening that changes the library's security posture — don't do
  it without a new explicit opt-in flag.
- Numbers in JSON are stored as source text (precision) — keep it.
- Front matter: stripped by default on md→html / md→text, renderable
  as a table with `{"front_matter":"table"}`, and always readable via
  `dc_front_matter_json`. Malformed front matter stays in the
  document — never dropped silently. Unknown options are
  INVALID_ARG, not ignored.
- Reference link definitions are collected before block parsing
  (skipping fenced code) and normalized with `normalize_label`
  (trim + collapse whitespace + lowercase). Labels are looked up at
  inline-render time, so `Refs` is threaded through every renderer.
- The MCP server lives in `src/mcp.rs` (protocol, tools, unit tests)
  with the thin stdio entry point in `src/mcp_main.rs`
  (`docconvert-mcp.exe`). It calls the library directly — never spawn
  the CLI — and must stay dependency-free too: no MCP SDK. stdout is
  the protocol stream and nothing else; diagnostics go to stderr. Tool
  failures are `isError: true` results carrying the `DC_*` code, while
  protocol faults are JSON-RPC errors. `tests/mcp.rs` drives the real
  binary through `CARGO_BIN_EXE_docconvert-mcp`.
- The CLI lives in `src/main.rs` and must stay dependency-free: the
  hand-rolled parser there is the whole of it. It is what local AI
  agents use, so keep it boring and machine-friendly: JSON
  introspection (`convert::formats_json` / `matrix_json` are the shared
  builders — never duplicate the lists in the CLI), byte-faithful
  output, silence on success, stderr-only errors, and exit codes equal
  to the `DC_*` codes. `tests/cli.rs` drives the real binary via
  `CARGO_BIN_EXE_docconvert`, including every documented exit code.
- Footnotes are document-wide: `collect_definitions` strips
  `[^label]: note` lines (with indented continuations),
  `collect_notes` numbers them by first reference, and the numbering
  reaches the inline renderer through `Notes`, which `render_block`
  and `inline_ctx` thread everywhere. `inline_with_refs` stays the
  public entry point and passes an empty `Notes`.
- Tables go through one row model: `parse_csv`/`parse_tsv` and
  `html::html_rows` / `markdown::markdown_rows` produce rows, and
  `rows_to_json`, `rows_to_markdown_table`, `rows_to_html_table`,
  `write_csv` and `write_tsv` render them. Add a format by adding a
  reader or writer there, never by re-parsing a string in convert.rs.
  TSV is the CSV grammar with a tab delimiter — cells containing a tab
  are quoted.
- The table extractors refuse documents without exactly one table;
  never let them guess rows out of prose.
- html→markdown rebuilds footnotes from the shape DocConvert emits
  (`<sup><a href="#fn-1">` and `<section class="footnotes">`);
  `note_text` drops the `#fnref-…` back-links so they are not carried
  into the Markdown.
- Markdown link titles are parsed once, by `split_destination` /
  `parse_title` in `src/markdown.rs`, and rendered as an escaped
  `title="…"`; `md_title` in `src/html.rs` writes them back. Task
  lists are recognized by `task_marker` (a `[ ]`/`[x]` prefix on an
  item's first paragraph) so the marker never leaks into the rendered
  text — keep that check in the list renderer's first-block path.
- Byte offsets in the Markdown block parser must go through
  `line_indent` / `tail_from` — slicing a line at a raw byte column
  can panic mid-UTF-8.
- Gate: WinDevKit tools/gate.sh (fmt + clippy -D warnings BOTH
  targets + FULL tests). This crate's match-heavy code attracts
  collapsible-match lints; prefer guards from the start.

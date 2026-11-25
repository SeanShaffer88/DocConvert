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
- Heading anchors have one owner: `markdown::slugify` plus `Slugs`
  (GitHub style — lowercase, punctuation dropped, whitespace runs to
  `-`, repeats `-1`, `-2`). `markdown::headings` and `html::headings`
  feed `toc::entries`, and the same `Slugs` run gives md→html its
  `id="…"` when `heading_ids` is on. Never re-implement a slug rule
  anywhere else; the TOC must keep pointing at the ids the conversion
  writes.
- `toc` is a view over headings, not a matrix pair: sources without
  headings are NOT_SUPPORTED. `convert::MATRIX` is 37 pairs;
  markdown→markdown remains a byte-faithful identity.
- `markdown::normalize` re-renders blocks canonically and must stay
  idempotent: definitions go last, inline prose is left as written, and
  `render_markdown_block` is the single place block shapes are printed.
- Byte offsets in the Markdown block parser must go through
  `line_indent` / `tail_from` — slicing a line at a raw byte column
  can panic mid-UTF-8.
- `src/toc.rs` holds the TOC renderers (markdown / html / json) plus
  `depths`, which turns heading levels into nesting; the `<ul>`/`<li>`
  writer must stay balanced — the tests count tags for that reason.
- The three document readers (`src/rst.rs`, `src/asciidoc.rs`,
  `src/opml.rs`) all land as **Markdown** and are then rendered by the
  Markdown renderers — that is why `rst → html` and `rst → markdown`
  agree. Never render HTML or text straight out of a reader; add the
  construct to the Markdown mapping instead.
- OPML reads through `html::parse_html` (it is XML of the same shape;
  `Node` and `parse_html` are `pub(crate)` for that reason) and writes
  attributes through `opml::escape_attr`. A document's outline is its
  headings: `markdown → opml` and `html → opml` both go through
  `opml::from_headings`.
- `normalize` canonicalizes inline markup through `normalize_inline`
  (asterisk emphasis, minimal code spans) but leaves paragraph line
  breaks alone — they are editorial. It must stay idempotent.
- Streaming exists only where it is exact: `platform::copy_file` is used
  by `convert::convert_file_streaming` for identity conversions of the
  byte-faithful formats (`BYTE_FAITHFUL`). Everything else parses the
  whole document, because partial parsing would be a lie. Formats live
  in `FORMATS`, pairs in `MATRIX`, aliases in `normalize_format`.
- One GFM table writer: `markdown::table`. The CSV/TSV → Markdown pairs
  and both readers call it, so a table's header row and escaping have a
  single definition. `csv::write_record` likewise backs `write_delimited`
  and the record streamer.
- `normalize`'s style layer is `markdown::Style` (`Emphasis`, `Fence`),
  parsed from the CLI flags, the MCP arguments and `dc_normalize`'s
  options object by the same `parse` functions. It changes the marker
  characters only — never the structure — and must stay idempotent.
  Fences come from `fence_for`, which grows past any run inside the body.
- `src/stream.rs` is the only place that writes an output file
  incrementally, and its rule is "identical or not at all": record
  streaming splits quote-aware and hands each record to `parse_csv`/
  `parse_tsv`; Markdown streaming renders one block at a time but
  restarts from the whole file the moment it meets a reference
  definition, a footnote definition or leading front matter
  (`markdown::has_document_wide_constructs`). Every streamed path has a
  test asserting equality with its whole-document reference — keep it
  that way before adding a pair.
- The release number is written once, in `Cargo.toml`. `lib.rs` derives
  `VERSION` with `env!("CARGO_PKG_VERSION")`, `dc_version` does the same,
  and `tests/file_io.rs::version_is_one_source_of_truth` pins both the
  numeric `VERSION_*` parts and the banner line in `include/docconvert.h`
  to it. A bump therefore fails CI until the header agrees — never
  hardcode a version string again.
- `platform::console_is_fresh()` (`GetConsoleProcessList`, one attached
  process) exists only so a double-click does not flash a window and
  vanish. `platform::double_clicked(args)` combines it with "no
  arguments", and both binaries print a notice and wait for Enter. Never
  widen it: a typed command or a script must never block on stdin, so
  the predicate stays "no arguments **and** a console that exists for us
  alone", and non-Windows always answers false.
- The exe icon is a resource, not a dependency: `build_support/win_icon.rs`
  (included by `build.rs` and by `tests/windows_icon.rs`) turns
  `assets/docconvert.ico` into a `.rsrc` COFF object and hands it to the
  linker with `rustc-link-arg-bins`/`-cdylib`. If you change the icon,
  regenerate the `.ico` with all six sizes and keep the test green — it
  reads the section back per the PE spec, which is the only check that can
  run before Windows links it.
- Tables: `markdown::table` is the only GFM writer, and GFM has no spans.
  Both readers flatten instead of refusing: a spanning cell keeps its text
  where the span starts and the covered cells are empty, wrapped cells
  join with single spaces, and column widths are advisory. rST reads its
  grid column-by-column (one open cell per column, closed by a rule
  segment) so spans fall out of the same loop; AsciiDoc reads cell
  specifiers before each `|` and reserves the cells a row span covers.
  Empty cells are kept — dropping them shifts every column after them.
- Streaming covers the table pairs too, and the same rule applies:
  identical output or fall back. A ragged CSV cannot become one GFM table
  without its widest row, so the record streamer returns `None` and the
  whole-document path writes the file.
- `--style` (and `style` in the MCP tool and in `dc_normalize` options) is
  `Style::parse_spec`: comma- or space-separated tokens, later ones win,
  `github` resets to the defaults. It is sugar over `--emphasis`/`--fence`,
  not a third setting — never let the two disagree.
- Gate: WinDevKit tools/gate.sh (fmt + clippy -D warnings BOTH
  targets + FULL tests). This crate's match-heavy code attracts
  collapsible-match lints; prefer guards from the start.

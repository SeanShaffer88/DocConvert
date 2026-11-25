# Changelog

All notable changes to DocConvert are recorded here. The format
follows Keep a Changelog, and the project adheres to Semantic
Versioning for the C ABI (`dc_*` symbols, error codes).

## [0.7.0] — 2026-11-25

### Added

- **TSV as a first-class format** (previously an alias of CSV): the
  RFC 4180 grammar with a tab for the comma, so a cell containing a tab
  is quoted and stays lossless. `tsv`, `tab` and `.tsv` are recognized,
  and **TSV mirrors CSV everywhere** — csv↔tsv, tsv→json, json→tsv,
  tsv→markdown, tsv→html, tsv→text, html→tsv, markdown→tsv — bringing
  the matrix to 25 pairs.
- One row model behind it: `parse_csv`/`parse_tsv`,
  `html::html_rows` and `markdown::markdown_rows` feed
  `rows_to_json` / `rows_to_markdown_table` / `rows_to_html_table` and
  the two writers, so CSV, TSV and JSON table conversions cannot drift
  apart.
- **Footnote back-references in html→markdown**: a `<sup>` wrapping an
  `#fn-…` anchor becomes `[^n]`, and a `<section class="footnotes">`
  block (the one DocConvert emits, or anyone else's in the same shape)
  becomes `[^n]: note` definitions with the back-link dropped — so a
  Markdown document with footnotes now round-trips through HTML.

### Changed

- `html→csv` error text now names all three table targets.
- Version 0.7.0 across the crate, header, and release.

## [0.6.0] — 2026-11-25

### Added

- **Markdown footnotes**: `[^label]` references and `[^label]: note`
  definitions (with indented continuation lines) are collected
  document-wide and numbered by first reference, as GFM does. HTML
  gets `<sup><a href="#fn-1" id="fnref-1">1</a></sup>` plus a
  `<section class="footnotes">` block with back-links; the text flavor
  numbers them inline and lists them at the end. Undefined notes and
  references inside fenced code stay literal.
- **html→json**: the first `<table>` as an array of objects, sharing
  html→csv's extraction and csv→json's typing (numbers, booleans and
  nulls, leading-zero codes kept as strings), so the two siblings
  agree cell for cell.
- **markdown→csv**: a document that is one GFM table — optionally
  under a single heading, the README-payload shape — becomes CSV with
  header row, cells flattened from Markdown to plain text. Prose, no
  table, two tables, or anything else is `DC_ERR_CORRUPT` rather than
  a silent guess.
- The matrix is now 17 pairs; the CLI, MCP server, `dc_matrix_json()`
  and docs pick the two new ones up from `convert::MATRIX`.

### Changed

- Version 0.6.0 across the crate, header, and release.

## [0.5.0] — 2026-11-25

### Added

- **Link titles**: `[text](url "title")`, `'title'` and `(title)` — on
  inline links, images, and reference definitions — now reach the HTML
  as `title="…"`, escaped like any other markup. Titles containing
  escaped delimiters (`"say \"hi\""`) parse correctly, and
  `<url with spaces>` destinations are supported.
- **GFM task lists**: `- [ ] todo` / `- [x] done` render as
  `<input type="checkbox" disabled[ checked]>` inside `<li>` (ordered
  lists too), and as `- [ ]` / `- [x]` in the text flavor.
- Both directions: html→markdown emits `[text](url "title")` and
  turns a leading checkbox back into `- [x]`, so a Markdown document
  round-trips through HTML without losing titles or task state.
- Tests for every delimiter, escaping, angle-bracket destinations, the
  text flavor, nested/ordered task items, and the full round trip.

### Changed

- Version 0.5.0 across the crate, header, and release.

## [0.4.0] — 2026-11-25

### Added

- **MCP server** (`docconvert-mcp.exe`): DocConvert as Model Context
  Protocol tools over stdio (newline-delimited JSON-RPC 2.0), linking
  the library directly rather than shelling out to the CLI.
  - tools `convert`, `convert_file`, `front_matter`, `formats`,
    `matrix`, `capabilities`;
  - protocol revision 2025-06-18, with 2024-11-05 and 2025-03-26
    accepted during `initialize`; `ping` supported, notifications left
    unanswered, unknown methods answered with −32601;
  - conversion failures become `isError: true` tool results carrying
    the `DC_*` code, while protocol faults stay JSON-RPC errors;
  - hand-rolled protocol plus the crate's own JSON parser — the
    dependency count is still zero.
- `tests/mcp.rs`: a real session against the spawned binary —
  handshake, notification handling, `tools/list`, every tool,
  tool-level and protocol-level errors, then a surviving `ping`.
- Unit tests inside `src/mcp.rs` for framing, dispatch and arguments.

### Changed

- The `capabilities` table now reports the codes under `error_codes`
  rather than `exit_codes`, since they are no longer only exit
  statuses; `convert::error_codes_json()` is the single source.
- `FrontMatterMode::parse` backs `--front-matter` and the
  `front_matter` option, so both spellings agree.
- Release packages now include `docconvert-mcp.exe`.
- Version 0.4.0 across the crate, header, and release.

## [0.3.0] — 2026-11-25

### Added

- **`docconvert` CLI** (`docconvert.exe`), built for scripts and local
  AI agents: `convert`, `frontmatter`, `formats`, `matrix`,
  `capabilities`, `version`, `help`.
  - stdin/stdout piping (`-` or an omitted path), byte-faithful output,
    silent success;
  - formats inferred from file extensions, with an explicit
    `--from`/`--to` requirement when a stream has no extension;
  - `--front-matter strip|table` and `--options '{json}'`, plus
    `--flag=value` forms;
  - exit codes equal to the `DC_*` codes (0–7), errors on stderr as
    `docconvert: <message>`;
  - `capabilities` returns formats, pairs, option values, commands and
    exit-code meanings as one JSON object — one call to plan work.
- `tests/cli.rs`: the binary driven through pipes, introspection JSON,
  file conversions and every documented exit code.
- Release packages now include `docconvert.exe` and the C example.

### Changed

- `convert::formats_json()` and `convert::matrix_json()` are the shared
  source for the ABI (`dc_formats_json`, `dc_matrix_json`) and the CLI,
  so all three describe the build identically.
- Version 0.3.0 across the crate, header, and release.

## [0.2.0] — 2026-11-25

### Added

- **Reference-style links** in Markdown: `[text][ref]`, `[text][]`,
  `[text]`, `![alt][ref]`, and `[label]: url "title"` definitions.
  Definitions are document-wide, are not picked up inside fenced code,
  and unused ones are dropped, as CommonMark specifies.
- **YAML front matter**: a leading `---` block is parsed (flat keys,
  nested maps, `- item` lists, inline `[a, b]` lists, `|` / `>`
  block scalars) and, for md→html / md→text, either stripped
  (default) or rendered as a key/value table via the new option
  `{"front_matter":"table"}`.
- **HTML→CSV**: the first `<table>` of a page becomes RFC 4180 CSV.
  Now part of the conversion matrix (`dc_matrix_json`).
- ABI: `dc_convert_opts`, `dc_convert_file_opts`, and
  `dc_front_matter_json`.
- `examples/convert_demo.c` — a complete tour of the ABI.
- Integration tests for the file API (`tests/file_io.rs`), a
  front-matter/reference-link fixture, and a test asserting the
  conversion matrix is exactly what dispatch supports.

### Changed

- `convert::MATRIX` is the single source of truth for the supported
  pairs; `dc_matrix_json()` is generated from it (it previously
  carried its own copy of the list).
- The `NOT_SUPPORTED` message no longer names a version.
- List handling snaps indentation to UTF-8 character boundaries, so a
  line indented with non-ASCII whitespace can no longer panic the
  Markdown parser.

### Fixed

- `dc_convert_file` reports `DC_ERR_CORRUPT` for non-UTF-8 input and
  writes nothing when a conversion fails.

## [0.1.0] — 2026-10-23

### Added

- Initial release: Markdown→HTML/text, HTML→Markdown/text,
  text→HTML/Markdown, CSV↔JSON, CSV→Markdown/HTML/text,
  JSON→text/HTML/Markdown, identity conversions, and the `dc_*` ABI.

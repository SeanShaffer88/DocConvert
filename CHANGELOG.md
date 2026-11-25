# Changelog

All notable changes to DocConvert are recorded here. The format
follows Keep a Changelog, and the project adheres to Semantic
Versioning for the C ABI (`dc_*` symbols, error codes).

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

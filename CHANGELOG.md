# Changelog

All notable changes to DocConvert are recorded here. The format
follows Keep a Changelog, and the project adheres to Semantic
Versioning for the C ABI (`dc_*` symbols, error codes).

## [0.10.2] — 2026-11-25

### Added

- **The product icon, compiled in.** `docconvert.exe`,
  `docconvert-mcp.exe` and `docconvert.dll` now carry the document mark
  as a Windows `RT_GROUP_ICON`/`RT_ICON` resource, so Explorer, the
  taskbar and Alt-Tab show it instead of the generic console glyph. The
  `.ico` holds 16, 24, 32, 48, 64 (BMP) and 256 (PNG) variants.
- `build_support/win_icon.rs` writes that resource itself: an `.ico` is
  turned into a `.rsrc` section and wrapped in a COFF object that the
  linker takes as an ordinary input — no `rc.exe` lookup, no
  `embed-resource`-style dependency, nothing to install on the runner.
  Non-Windows targets skip it entirely.
- `tests/windows_icon.rs` parses the section back from the PE/COFF
  specification — type table, name tables, language tables, data
  entries, relocations, symbol table — and asserts every image arrives
  byte for byte, so a wrong offset fails on Linux before a Windows
  linker sees it.
- The release zip carries `docconvert.ico` as well, for shortcuts.

## [0.10.1] — 2026-11-25

### Added

- **Double-click notices.** `docconvert.exe` and `docconvert-mcp.exe`
  are console programs with no GUI, so double-clicking them used to
  flash a window and exit (the CLI) or sit silently on stdin (the
  server). Both now detect a console created for them alone and print a
  short card — what the program is, the four commands worth knowing, or
  the `mcpServers` snippet — then wait for Enter. Detection is
  `GetConsoleProcessList` on Windows (one attached process = a
  double-click); a shell keeps its own process attached, so a script or
  a typed command never pauses, and non-Windows builds never pause at
  all. The notice text and the "is this a double-click?" decision are
  unit-tested; the Windows console probe itself is verified by hand.
- README: a **there is no GUI** note in the CLI section, plus the
  unsigned-binary / *mark of the web* steps (`Unblock-File`) that
  otherwise look exactly like "the program does not open".

### Changed

- The release number is written once, in `Cargo.toml`: `VERSION` now
  derives from `CARGO_PKG_VERSION` (as `dc_version` already did), and
  `tests/file_io.rs::version_is_one_source_of_truth` fails unless the
  numeric `VERSION_*` parts and the `DocConvert.dll vX.Y.Z` banner in
  `include/docconvert.h` match it. No behaviour change.
- CI can be re-run on demand: `ci.yml` gained `workflow_dispatch`, so a
  branch's gate can be started without a new push.

## [0.10.0] — 2026-11-25

### Added

- **Tables in the document readers**: reStructuredText **simple and
  grid** tables and AsciiDoc `|===` tables become GFM tables (header
  rows included), so `rst → html`, `asciidoc → text` and friends carry
  tables through — single-line cells only, no spans.
- **Normalize style layer**: `normalize` takes an optional style —
  `emphasis` (`asterisk` default, or `underscore`) and `fence`
  (`backtick` default, or `tilde`) — on the CLI, the MCP tool and the
  new `dc_normalize` ABI call. Fences grow when the code contains them,
  so a block can never close early.
- **`dc_normalize(input, options_json)`**: canonical Markdown at the ABI
  level, previously CLI/MCP only.
- **Block-level streaming**: `dc_convert_file_streaming` (MCP `stream`,
  CLI `--stream`) now streams `text → markdown|html` per paragraph and
  `csv|tsv → csv|tsv|text` per RFC 4180 record, each split quote-aware
  and parsed by the same functions as the whole-file path, plus
  `markdown → html|text` per block. Markdown watches for the
  document-wide constructs (reference definitions, footnote definitions,
  leading front matter) and redoes the conversion from the whole file
  when it meets one, so streamed output is identical to the
  whole-document output.

### Changed

- `csv::write_record` is public and `write_delimited` is built from it,
  so a streamed record and a whole-file write cannot quote differently.
- `markdown::table` is the one GFM table writer (the CSV/TSV → Markdown
  pairs use it too).
- Version 0.10.0 across the crate, header, and release.

## [0.9.0] — 2026-11-25

### Added

- **reStructuredText (subset) as a source format**: sections by
  adornment order (underlined and overlined), bullet and enumerated
  lists with indentation nesting, literal blocks after `::`, block
  quotes, transitions, comments and directives (dropped with their
  bodies), field lists, and inline literals, emphasis, strong and
  backtick links with roles. `rst → markdown|html|text`.
- **AsciiDoc (subset) as a source format**: `= Title` sections,
  `*`/`**` and `.`/`..` lists, `----`/`....` blocks with an optional
  `[source,lang]`, example and quote blocks, comments, attribute and
  block-attribute lines (dropped), admonitions, `'''` breaks,
  description lists, inline macros. `asciidoc → markdown|html|text`.
- **OPML both ways**: `opml → markdown|html|text` reads an outline as
  nested bullets (`_note` becomes the item's text, existing ids are
  kept), and `markdown → opml` / `html → opml` write a document's
  headings as an escaped `<outline>` tree.
- Both readers land as Markdown and are then rendered by the Markdown
  renderers, so their Markdown, HTML and text outputs cannot drift
  apart; `toc` reads them too, with the same anchors.
- **Streaming file API**: `dc_convert_file_streaming`, the MCP
  `convert_file` `stream` flag and the CLI `--stream` copy an identity
  conversion (markdown, html, text, rst, asciidoc, opml — byte-faithful
  by definition) in 64 KiB chunks; every other pair converts through the
  whole-buffer path, because those conversions need the whole document.
- **Inline normalization** in `normalize`: `_emphasis_`/`__strong__`
  become the asterisk forms and code spans lose surplus backticks and
  padding. Line breaks inside a paragraph stay as written.

### Changed

- The matrix is **37 pairs**: the eleven new ones are the readers and
  the OPML writer above. Formats are `markdown, html, csv, tsv, json,
  text, rst, asciidoc, opml`; extensions `.rst`, `.rest`, `.adoc`,
  `.asciidoc` and `.opml` are detected.
- Version 0.9.0 across the crate, header, and release.

## [0.8.0] — 2026-11-25

### Added

- **Table of contents generator** (`toc`, `dc_toc_json`, the `toc` MCP
  tool): the headings of a Markdown or HTML document as a nested
  Markdown list, nested `<ul>`/`<li>` markup, or a JSON array of
  `{level, text, anchor}`. Anchors follow GitHub — lowercase,
  punctuation dropped, whitespace runs collapsed to `-`, repeats
  numbered `-1`, `-2` — and HTML input keeps an `id` the document
  already carries. Sources without headings (CSV, TSV, JSON, text) are
  `NOT_SUPPORTED` rather than empty output.
- **`heading_ids` option** (CLI `--heading-ids`): Markdown→HTML writes
  `<h2 id="slug">` using the same anchors `toc` reports, so a generated
  table of contents links to ids the conversion actually produced.
  Off by default, so the CommonMark-shaped output is unchanged.
- **Markdown→Markdown normalization** (CLI `normalize`, the
  `normalize` MCP tool): canonical block structure — one blank line
  between blocks, `#` headings, `-` bullets, sequential ordered
  numbers, GFM tables with their alignment row, `> ` quotes, `---`
  rules — with link and footnote definitions collected at the end.
  Inline prose is left as written, and the pass is idempotent.
- `markdown::headings`, `markdown::slugify`, `markdown::Slugs` and
  `html::headings` are public: one anchor model shared by headings,
  the TOC and the `heading_ids` output.

### Changed

- `convert::Options` gained `heading_ids` (JSON `true`/`false`, parsed
  strictly — any other value is `INVALID_ARG`), and `capabilities`
  lists it next to `front_matter`.
- `markdown→markdown` stays a byte-faithful identity: normalizing is
  opt-in, never a side effect of converting.
- Version 0.8.0 across the crate, header, and release.

## [0.7.0] — 2026-11-25

### Added

- **TSV as a first-class format** (previously an alias of CSV): the
  RFC 4180 grammar with a tab for the comma, so a cell containing a tab
  is quoted and stays lossless. `tsv`, `tab` and `.tsv` are recognized,
  and **TSV mirrors CSV everywhere** — csv↔tsv, tsv→json, json→tsv,
  tsv→markdown, tsv→html, tsv→text, html→tsv, markdown→tsv — bringing
  the matrix to 26 pairs.
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

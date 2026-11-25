# DocConvert roadmap

## Shipped in v0.10 — the candidates

- **rST and AsciiDoc tables**: simple and grid tables, and `|===`
  tables, become GFM tables — single-line cells, no spans.
- **Emphasis style options for `normalize`**: `--emphasis
  asterisk|underscore` and `--fence backtick|tilde`, on every surface
  including the new `dc_normalize` ABI call.
- **Streaming for more than identity**: text per paragraph, CSV/TSV per
  record, Markdown per block, each proven equal to the whole-document
  path (Markdown restarts from the file when it meets a reference or
  footnote definition).

## Shipped in v0.9 — the roadmap, completed

- **reStructuredText (subset)** as a source: sections by adornment
  order, lists with indentation nesting, literal blocks, quotes,
  transitions, comments/directives/field lists dropped, and the inline
  subset including backtick links and roles. `rst → markdown|html|text`.
- **AsciiDoc (subset)** as a source: `=` sections, `*`/`.` lists,
  delimited blocks with `[source,lang]`, comments and attribute lines
  dropped, admonitions, description lists, inline macros.
  `asciidoc → markdown|html|text`.
- **OPML, both ways**: an outline becomes nested bullets
  (`opml → markdown|html|text`), and a document's headings become an
  escaped `<outline>` tree (`markdown → opml`, `html → opml`).
- **Inline normalization**: `_emphasis_`/`__strong__` become the
  asterisk forms, code spans lose surplus backticks and padding.
- **Streaming file API**: `dc_convert_file_streaming` (plus the MCP
  `stream` flag and the CLI `--stream`) copies identity conversions in
  64 KiB chunks; every other pair keeps the whole-buffer path, which is
  what correctness requires.

Both readers land as Markdown and are rendered by the Markdown
renderers, so a document's Markdown, HTML and text never disagree; `toc`
reads them with the same anchors. The matrix is 37 pairs.

## Next

Nothing is committed here: the roadmap as written is complete. The
candidates below are deliberately small and only worth taking if they
earn their keep.

- **rST and AsciiDoc tables** — grid/simple tables and `|===` would
  extend both readers to the one construct they still refuse.
- **Emphasis style options for `normalize`** — an opt-in `*`/`_` and
  code-fence style layer on top of today's canonical form.
- **Streaming for more than identity** — only possible with a
  block-at-a-time Markdown pipeline; the string API stays the contract.

## Later

- More formats behind the same matrix shape: AsciiDoc (full), DocBook,
  MediaWiki.

## Shipped in v0.8

- **Table of contents generator** — the headings of a Markdown or HTML
  document as nested links (`markdown`, `html` or `json`), anchored the
  way GitHub does, and a `heading_ids` option so Markdown→HTML writes
  those same `id`s onto its headings. Exposed as `docconvert toc`,
  `dc_toc_json` and the `toc` MCP tool.
- **Markdown→Markdown normalization** — canonical blocks (headings,
  bullets, ordered numbers, tables with alignment, quotes, rules,
  definitions last) with inline prose untouched, idempotent, as
  `docconvert normalize` and the `normalize` MCP tool. Converting
  markdown→markdown stays a byte-faithful identity.

## Shipped in v0.7

- **TSV as a first-class format**: the CSV grammar with a tab for the
  comma (cells containing a tab are quoted, so nothing is lost), and
  the rule that wherever CSV is a source or target, TSV is too — nine
  new pairs, 26 in total.
- **Footnote back-references in html→markdown**: `<sup>` + `#fn-…`
  becomes `[^n]`, and a `<section class="footnotes">` block becomes
  `[^n]: note` definitions, so footnotes survive the round trip.

## Shipped in v0.6

- **Footnotes** — the last common GFM construct: `[^label]` references
  and `[^label]: note` definitions, numbered by first reference, with
  a footnotes section in HTML and a numbered list in the text flavor.
- **html→json** — the first table as an array of objects, built on the
  same extraction as html→csv, so the two never disagree.
- **markdown→csv** — a document that is exactly one table (optionally
  under a heading) becomes CSV; anything with prose is refused.

## Shipped in v0.5

- **Link titles** end to end: `"…"`, `'…'` and `(…)` titles on inline
  links, images and reference definitions reach the HTML as
  `title="…"` (escaped), and html→markdown writes them back out.
- **GFM task lists**: `- [ ]` / `- [x]` become checkboxes in HTML and
  stay readable in the text flavor; html→markdown turns a leading
  checkbox back into a task item, so documents round-trip.

## Shipped in v0.4

- **MCP server** (`docconvert-mcp.exe`): the same conversions as Model
  Context Protocol tools over stdio, for local AI clients that would
  otherwise shell out to the CLI. Tool errors carry the `DC_*` code so
  a model can tell "unsupported pair" from "file missing"; the protocol
  and the JSON are still hand-written, so nothing new is pulled in.
- Introspection is shared three ways now: the ABI, the CLI and the MCP
  server all describe the build from `convert::*_json`.

## Shipped in v0.3

- **Command line front end** (`docconvert.exe`) with the same matrix:
  pipe-friendly `convert`, `frontmatter`, JSON introspection
  (`formats`, `matrix`, and a single `capabilities` object), and exit
  codes that mirror the `DC_*` codes. Formats are inferred from file
  extensions; stdin/stdout require an explicit format instead of a
  guess.
- One JSON description (`convert::formats_json` / `matrix_json`)
  shared by the ABI and the CLI.

## Shipped in v0.2

- **Reference-style links** and link definitions in the Markdown
  parser: `[text][ref]`, `[text][]`, `[text]`, `![alt][ref]`, with
  definitions collected document-wide (they are skipped inside
  fenced code and unused definitions disappear).
- **YAML front matter**: parsed as a JSON-able subset, stripped from
  md→html / md→text by default, renderable as a table with
  `{"front_matter":"table"}`, and readable at any time through
  `dc_front_matter_json`.
- **HTML→CSV** for documents whose first table is the payload — the
  former NOT_SUPPORTED pair is now a real one, with RFC 4180 quoting
  and `DC_ERR_CORRUPT` when there is no table.
- ABI: `dc_convert_opts`, `dc_convert_file_opts`,
  `dc_front_matter_json`; one `convert::MATRIX` drives dispatch and
  `dc_matrix_json()` alike.

## Next

- **reStructuredText (subset)** as a second portable-document source —
  headings, lists, literal blocks and links — behind the same matrix
  shape and the same heading/`toc` machinery.
- **Emphasis and inline normalization** for `normalize`: the style axis
  (`*`/`_`, code fences, line wrapping) that v0.8 deliberately left
  alone, opt-in so structure stays the default.
- **Streaming `convert_file`** for inputs too large to hold twice (the
  string API stays the default and the ABI contract).

## Later

- More formats behind the same matrix shape: AsciiDoc (subset), OPML.

## Non-goals

- No layout formats (DOCX/PDF) — those are real document models,
  not text conversions; PDF belongs to PDFKit.

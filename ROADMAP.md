# DocConvert roadmap

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

- **Table of contents generator** for Markdown and HTML inputs —
  headings as nested links, anchored the way the document's own
  headings are.
- **Markdown→Markdown normalization**: canonical formatting
  (list markers, table alignment, emphasis style) so converted output
  is diffable.

## Later

- More formats behind the same matrix shape: AsciiDoc (subset),
  reStructuredText (subset), OPML, and TSV as a first-class format
  rather than a CSV alias.
- Streaming API for very large files (the string API is whole-input
  by design).

## Non-goals

- No layout formats (DOCX/PDF) — those are real document models,
  not text conversions; PDF belongs to PDFKit.

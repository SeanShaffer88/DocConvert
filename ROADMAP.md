# DocConvert roadmap

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

- **Footnotes** (`[^1]`) — the last common GFM construct still
  missing from the Markdown reader.
- **HTML→JSON** as a table-array equivalent of html→csv, sharing the
  same first-table extraction.
- **`markdown→csv`** when the document is exactly one table (common
  for README payloads), reusing the table renderer.

## Later

- More formats behind the same matrix shape: AsciiDoc (subset),
  reStructuredText (subset), OPML, and TSV as a first-class format
  rather than a CSV alias.
- Streaming API for very large files (the string API is whole-input
  by design).
- Markdown→Markdown normalization (canonical formatting), and a
  table-of-contents generator for Markdown/HTML inputs.

## Non-goals

- No layout formats (DOCX/PDF) — those are real document models,
  not text conversions; PDF belongs to PDFKit.

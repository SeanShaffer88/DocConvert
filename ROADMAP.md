# DocConvert roadmap

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

- **Link titles** emitted on the Markdown→HTML path (`<a title=…>`),
  parsed today and dropped.
- **Task lists** (`- [x]`) and footnotes — the two remaining common
  GFM constructs.
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

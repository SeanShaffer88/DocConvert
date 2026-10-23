# DocConvert roadmap

## Next

- **Reference-style links** and link definitions in the Markdown
  parser (the most common gap vs full CommonMark).
- **YAML front matter** pass-through/strip option for md→html.
- **HTML→CSV** for documents whose first table is the payload —
  turns the current NOT_SUPPORTED pair into a useful one.

## Later

- More formats behind the same matrix shape: AsciiDoc (subset),
  reStructuredText (subset), OPML, and TSV as a first-class format
  rather than a CSV alias.
- Streaming API for very large files (the string API is whole-input
  by design in v0.1).
- Markdown→Markdown normalization (canonical formatting), and a
  table-of-contents generator for Markdown/HTML inputs.

## Non-goals

- No layout formats (DOCX/PDF) — those are real document models,
  not text conversions; PDF belongs to PDFKit.

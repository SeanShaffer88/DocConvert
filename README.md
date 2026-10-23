# DocConvert

Document conversion between **Markdown, HTML, CSV, JSON, and plain
text** behind one clean C ABI — pure Rust, zero dependencies, every
parser hand-written. Library #16 in the Windows DLL Collection.

## ABI (`include/docconvert.h`, prefix `dc_`)

| Function | Purpose |
|---|---|
| `dc_convert(input, from, to)` | Convert a string; returns the result or null + error. |
| `dc_convert_file(in, out, from, to)` | Convert a file; null format arguments detect from file extensions. |
| `dc_formats_json()` | `["markdown","html","csv","json","text"]` |
| `dc_matrix_json()` | The supported `{from,to}` pairs, enumerable at runtime. |
| `dc_version*`, `dc_last_error*`, `dc_free_string` | Version info, thread-local errors, disposal. |

Format names accept aliases (`md`, `htm`, `txt`). Errors use the
shared 0–6 codes plus `DC_ERR_CORRUPT` (7) for parse failures, and
unsupported pairs are `DC_ERR_NOT_SUPPORTED` — never a lossy guess.

## The matrix (v0.1)

markdown→html, markdown→text, html→markdown, html→text,
text→html, text→markdown, csv→json, json→csv, csv→markdown (table),
csv→html (table), csv→text, json→text/html/markdown (pretty-printed,
fenced for Markdown), plus identity conversions that validate and
normalize (JSON pretty-prints, CSV re-quotes canonically).

## Format notes

- **Markdown**: ATX + setext headings, emphasis/strong/strike, code
  spans + fenced blocks (with `language-*` classes), links, images,
  autolinks, blockquotes, nested lists, horizontal rules, GFM pipe
  tables with alignment. **Raw HTML in Markdown is escaped**, so
  output is safe to embed by default.
- **HTML reader**: a forgiving DOM parser — entities (named +
  numeric), attributes, void elements; script/style contents are
  dropped. Tables become GFM tables.
- **CSV**: RFC 4180 quoting in both directions (embedded commas,
  quotes, newlines). CSV→JSON infers numbers/booleans/null — with
  one deliberate exception: leading-zero values like `007` stay
  strings, because they're codes, not quantities.
- **JSON**: full parser (key order preserved, numbers kept as
  source text so nothing loses precision, surrogate pairs handled)
  and compact/pretty serializers.

## Verification

- 13 unit tests + interop tests: every matrix highlight, RFC 4180
  round trips, JSON round trips (including re-parsing pretty
  output), HTML entity handling, and full md→html→md round trips.
- **External cross-check**: the sample document in
  `tests/fixtures/sample.md` was rendered by **marked** (a mainstream
  Markdown engine) and by DocConvert — the outputs are structurally
  identical (verified locally when the fixture was added, and the
  test pins our structure so regressions surface in CI).
- Gate (WinDevKit `tools/gate.sh`): fmt, clippy (host + Windows
  target) with `-D warnings`, full suite — green. Pure logic, no
  Win32 calls.

## Scope limits (v0.1)

Markdown: no reference-style links, no HTML pass-through, no
footnotes/task lists. HTML: expects roughly well-formed input (no
tag-soup repair heuristics). CSV→JSON always uses the first row as
headers. See ROADMAP.md.

## License

MIT © 2026 Sean Shaffer.

# DocConvert

Document conversion between **Markdown, HTML, CSV, JSON, and plain
text** behind one clean C ABI — pure Rust, zero dependencies, every
parser hand-written. Library #16 in the Windows DLL Collection.

## ABI (`include/docconvert.h`, prefix `dc_`)

| Function | Purpose |
|---|---|
| `dc_convert(input, from, to)` | Convert a string; returns the result or null + error. |
| `dc_convert_opts(input, from, to, options_json)` | Same, with options (`{"front_matter":"table"}`). |
| `dc_convert_file(in, out, from, to)` | Convert a file; null format arguments detect from file extensions. |
| `dc_convert_file_opts(in, out, from, to, options_json)` | Same, with options. |
| `dc_front_matter_json(input)` | The YAML front matter as JSON (`{}` when absent). |
| `dc_formats_json()` | `["markdown","html","csv","json","text"]` |
| `dc_matrix_json()` | The supported `{from,to}` pairs, enumerable at runtime. |
| `dc_version*`, `dc_last_error*`, `dc_free_string` | Version info, thread-local errors, disposal. |

Format names accept aliases (`md`, `htm`, `txt`). Errors use the
shared 0–6 codes plus `DC_ERR_CORRUPT` (7) for parse failures, and
unsupported pairs are `DC_ERR_NOT_SUPPORTED` — never a lossy guess.

```c
char *html = dc_convert("# Hi\n", "md", "html");
if (html == NULL) {
    fprintf(stderr, "error %d: %s\n", dc_last_error_code(), dc_last_error());
} else {
    puts(html);
    dc_free_string(html);   /* caller owns every returned string */
}
```

`examples/convert_demo.c` exercises the whole surface (it compiles
clean with `gcc -Wall -Wextra -Werror -Iinclude -c` and as C++).

## Options

`dc_convert_opts` / `dc_convert_file_opts` take a JSON object; `NULL`
or `""` means defaults. Unknown keys and values are
`DC_ERR_INVALID_ARG` — options are never ignored silently.

| Key | Values | Effect |
|---|---|---|
| `front_matter` | `"strip"` (default) | Drop the YAML block from md→html / md→text. |
| | `"table"` | Render it as a key/value table instead. |

The metadata is never lost either way: `dc_front_matter_json` returns
it as a JSON object regardless of how the document is converted.

## The matrix (v0.2)

markdown→html, markdown→text, html→markdown, html→text,
html→csv, text→html, text→markdown, csv→json, json→csv,
csv→markdown (table), csv→html (table), csv→text,
json→text/html/markdown (pretty-printed, fenced for Markdown), plus
identity conversions that validate and normalize (JSON pretty-prints,
CSV re-quotes canonically).

`convert::MATRIX` is the single source of truth: `dc_matrix_json()` is
generated from it, and `matrix_is_exact` asserts that every listed
pair converts and every unlisted one is `NOT_SUPPORTED`.

## Format notes

- **Markdown**: ATX + setext headings, emphasis/strong/strike, code
  spans + fenced blocks (with `language-*` classes), inline links and
  images, **reference-style links** (`[text][ref]`, `[text][]`,
  `[text]`, `![alt][ref]`) with `[ref]: url "title"` definitions
  collected document-wide, autolinks, blockquotes, nested lists,
  horizontal rules, GFM pipe tables with alignment. **Raw HTML in
  Markdown is escaped**, so output is safe to embed by default.
- **Front matter**: a leading `---` block is parsed as a YAML subset
  (flat keys, nested maps, `- item` lists, inline `[a, b]` lists,
  `|`/`>` block scalars). Numbers and booleans become JSON scalars,
  with the same guard as CSV→JSON: leading-zero codes like `007`
  stay strings. Malformed front matter is left in the document
  rather than silently swallowed.
- **HTML reader**: a forgiving DOM parser — entities (named +
  numeric), attributes, void elements; script/style contents are
  dropped. Tables become GFM tables, and **html→csv** lifts the first
  `<table>` out of a page (RFC 4180 quoting, cell text flattened;
  `colspan`/`rowspan` are not expanded). A page with no table is
  `DC_ERR_CORRUPT`, not an empty file.
- **CSV**: RFC 4180 quoting in both directions (embedded commas,
  quotes, newlines). CSV→JSON infers numbers/booleans/null — with
  one deliberate exception: leading-zero values like `007` stay
  strings, because they're codes, not quantities.
- **JSON**: full parser (key order preserved, numbers kept as
  source text so nothing loses precision, surrogate pairs handled)
  and compact/pretty serializers.

## Verification

- Unit suites per module (front matter, Markdown incl. reference
  links, HTML incl. html→csv, CSV, JSON, matrix, FFI) plus
  integration tests: fixture cross-checks in `tests/interop.rs` and
  the file API with its error codes in `tests/file_io.rs`.
- **External cross-check**: the sample document in
  `tests/fixtures/sample.md` was rendered by **marked** (a mainstream
  Markdown engine) and by DocConvert — the outputs are structurally
  identical (verified locally when the fixture was added, and the
  test pins our structure so regressions surface in CI).
- Gate (WinDevKit `tools/gate.sh`): fmt, clippy (host + Windows
  target) with `-D warnings`, full suite — green. Pure logic, no
  Win32 calls.

## Scope limits (v0.2)

Markdown: no HTML pass-through, no footnotes/task lists; link titles
are parsed but not emitted (Markdown→HTML keeps `href` only).
HTML: expects roughly well-formed input (no tag-soup repair
heuristics); html→csv uses the first table only and ignores spans.
CSV→JSON always uses the first row as headers. See ROADMAP.md.

## License

MIT © 2026 Sean Shaffer.

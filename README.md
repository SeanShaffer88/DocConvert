# DocConvert

Document conversion between **Markdown, HTML, CSV, JSON, and plain
text** behind one clean C ABI — pure Rust, zero dependencies, every
parser hand-written. Library #16 in the Windows DLL Collection. The
same matrix also ships as a CLI and as an MCP server for local AI
clients.

## Command line (`docconvert.exe`)

The same matrix, script-friendly: byte-faithful pipes, silent success,
and exit codes that *are* the `DC_*` codes — `$?` tells you exactly
what happened.

```console
$ docconvert convert -f md -t html README.md README.html
$ cat notes.md | docconvert convert -f md -t text
$ docconvert convert --from=md --to=html --front-matter=table post.md
$ docconvert frontmatter post.md
{"title":"Release 2.0","draft":false}
```

`capabilities` is the one call a local AI (or a script) needs before
converting anything: formats, supported pairs, option values, commands,
and the meaning of every exit code, all in one JSON object. `formats`
and `matrix` return those pieces alone.

| Command | Purpose |
|---|---|
| `convert [flags] [IN] [OUT]` | Convert; omitted or `-` paths mean stdin/stdout. |
| `frontmatter [IN]` | YAML front matter as JSON (`{}` when absent). |
| `formats`, `matrix`, `capabilities` | JSON introspection. |
| `version`, `help` | Version string, usage. |

Flags: `-f/--from`, `-t/--to`, `-i/--input`, `-o/--output`,
`--front-matter strip|table`, `--options '{...}'`; `--flag=value` is
accepted too. Formats fall back to the file extensions, and stdin or
stdout without an inferable format is an argument error rather than a
guess.

Exit codes: `0` ok, `1` invalid argument, `2` conversion not supported,
`3` file not found, `4` access denied, `5` platform error, `6` internal
error, `7` input failed to parse.

## MCP server (`docconvert-mcp.exe`)

DocConvert as tools for a local AI client, over the Model Context
Protocol (stdio, newline-delimited JSON-RPC 2.0). It links the library
directly instead of shelling out to `docconvert.exe`, and it still adds
no dependencies.

Register it with any MCP-capable client:

```json
{
  "mcpServers": {
    "docconvert": { "command": "C:\\path\\to\\docconvert-mcp.exe" }
  }
}
```

| Tool | Purpose |
|---|---|
| `convert` | `{input, from, to, front_matter?}` — the converted document as text. |
| `convert_file` | `{input_path, output_path, from?, to?, front_matter?}` — writes the file, returns `{written, bytes, from, to}`; formats come from the extensions. |
| `front_matter` | `{input}` or `{path}` — the YAML front matter as JSON (`{}` when absent). |
| `formats`, `matrix`, `capabilities` | The same JSON introspection the CLI prints. |

Failures are written for the model to read: a conversion problem comes
back as a tool result with `isError: true` and text like
`docconvert: markdown -> csv is not supported (code 2)`, the code being
the `DC_*` code and therefore the CLI's exit code. Protocol mistakes
(unknown method, unknown tool, malformed JSON) are plain JSON-RPC
errors. stdout carries protocol lines only.

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

## The matrix (v0.3)

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
  collected document-wide, **link titles** (`"…"`, `'…'`, `(…)`) on
  both inline and reference links and images, **GFM task lists**
  (`- [x] done`), autolinks, blockquotes, nested lists, horizontal
  rules, GFM pipe tables with alignment. **Raw HTML in Markdown is
  escaped**, so output is safe to embed by default.
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
  integration tests: fixture cross-checks in `tests/interop.rs`, the
  file API with its error codes in `tests/file_io.rs`, the CLI driven
  end to end through pipes, introspection JSON and exit codes in
  `tests/cli.rs`, and a full MCP session against the spawned server in
  `tests/mcp.rs`.
- **External cross-check**: the sample document in
  `tests/fixtures/sample.md` was rendered by **marked** (a mainstream
  Markdown engine) and by DocConvert — the outputs are structurally
  identical (verified locally when the fixture was added, and the
  test pins our structure so regressions surface in CI).
- Gate (WinDevKit `tools/gate.sh`): fmt, clippy (host + Windows
  target) with `-D warnings`, full suite — green. Pure logic, no
  Win32 calls.

## Scope limits (v0.5)

Markdown: no HTML pass-through, no footnotes; link titles reach the
HTML output (and come back through html→markdown) but are dropped from
the plain-text flavor, which keeps the label and URL only.
HTML: expects roughly well-formed input (no tag-soup repair
heuristics); html→csv uses the first table only and ignores spans.
CSV→JSON always uses the first row as headers. See ROADMAP.md.

## License

MIT © 2026 Sean Shaffer.

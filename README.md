# DocConvert

Document conversion between **Markdown, HTML, CSV, TSV, JSON, plain
text, reStructuredText, AsciiDoc, and OPML** behind one clean C ABI —
pure Rust, zero dependencies, every parser hand-written. Library #16 in the Windows DLL Collection. The
same matrix also ships as a CLI and as an MCP server for local AI
clients.

## Command line (`docconvert.exe`)

The same matrix, script-friendly: byte-faithful pipes, silent success,
and exit codes that *are* the `DC_*` codes — `$?` tells you exactly
what happened.

**There is no GUI, on purpose** — `docconvert.exe` is a console
program: it takes arguments, reads and writes files, and prints to the
terminal. Nothing happens if you double-click it, so it now says so and
waits for Enter instead of flashing a window and vanishing:

```
DocConvert 0.10.1 — a command-line converter. There is no window: it
reads files, writes files, and prints to the terminal.

  .\docconvert.exe help                           the full command list
  .\docconvert.exe convert notes.md notes.html    a file conversion

Press Enter to close this window.
```

Run it from PowerShell, `cmd`, or any terminal — `Shift`-right-click a
folder and choose *Open PowerShell window here*, then
`.\docconvert.exe capabilities`.

The release binaries are unsigned, so Windows may warn the first time,
and a zip that came through a browser carries the *mark of the web*:
`Unblock-File .\docconvert.exe` (or Properties → *Unblock*) clears it. A
console program never shows a window of its own — no window is not a
crash. `docconvert version` and `docconvert capabilities` are the two
fastest ways to prove a download works.

```console
$ docconvert convert -f md -t html README.md README.html
$ cat notes.md | docconvert convert -f md -t text
$ docconvert convert --from=md --to=html --front-matter=table post.md
$ docconvert frontmatter post.md
{"title":"Release 2.0","draft":false}
$ docconvert toc README.md
- [DocConvert](#docconvert)
  - [Command line](#command-line-docconvertexe)
$ docconvert normalize notes.md > canonical.md
$ docconvert convert -f rst -t html notes.rst notes.html
$ docconvert convert -f md -t opml README.md outline.opml
$ docconvert convert --stream -f md -t md big.md copy.md
$ docconvert normalize --emphasis underscore --fence tilde notes.md
```

`capabilities` is the one call a local AI (or a script) needs before
converting anything: formats, supported pairs, option values, commands,
and the meaning of every exit code, all in one JSON object. `formats`
and `matrix` return those pieces alone.

| Command | Purpose |
|---|---|
| `convert [flags] [IN] [OUT]` | Convert; omitted or `-` paths mean stdin/stdout. |
| `frontmatter [IN]` | YAML front matter as JSON (`{}` when absent). |
| `toc [flags] [IN]` | Table of contents (`--format markdown\|html\|json`, `--from md\|html`). |
| `normalize [IN] [OUT]` | Rewrite Markdown canonically (same document, diffable). |
| `formats`, `matrix`, `capabilities` | JSON introspection. |
| `version`, `help` | Version string, usage. |

Flags: `-f/--from`, `-t/--to`, `-i/--input`, `-o/--output`,
`--front-matter strip|table`, `--heading-ids`, `--stream` (file-to-file,
block by block), `--options '{...}'`; `normalize` adds
`--emphasis asterisk|underscore` and `--fence backtick|tilde`;
`--flag=value` is
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
| `convert_file` | `{input_path, output_path, from?, to?, front_matter?, heading_ids?, stream?}` — writes the file, returns `{written, bytes, from, to}`; formats come from the extensions. |
| `front_matter` | `{input}` or `{path}` — the YAML front matter as JSON (`{}` when absent). |
| `toc` | `{input\|path, from?, format?}` — the headings as a nested Markdown list, nested `<ul>`, or `[{level,text,anchor}]`. |
| `normalize` | `{input, emphasis?, fence?}` — the same Markdown, rewritten canonically. |
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
| `dc_convert_file_streaming(in, out, from, to)` | Bytes written, or negative; streams identity copies, text, CSV/TSV records and Markdown blocks. |
| `dc_front_matter_json(input)` | The YAML front matter as JSON (`{}` when absent). |
| `dc_toc_json(input, from)` | The headings as `[{"level":2,"text":"…","anchor":"…"}]` (Markdown or HTML input). |
| `dc_normalize(input, options_json)` | Canonical Markdown; options set `emphasis` and `fence`. |
| `dc_formats_json()` | `["markdown","html","csv","tsv","json","text","rst","asciidoc","opml"]` |
| `dc_matrix_json()` | The supported `{from,to}` pairs, enumerable at runtime. |
| `dc_version*`, `dc_last_error*`, `dc_free_string` | Version info, thread-local errors, disposal. |

Format names accept aliases (`md`, `htm`, `txt`, `rest`, `adoc`). Errors use the
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
| `heading_ids` | `false` (default) | Markdown-to-HTML headings carry no `id`. |
| | `true` | `<h2 id="slug">` — the anchors `toc` reports. |

The metadata is never lost either way: `dc_front_matter_json` returns
it as a JSON object regardless of how the document is converted.

## Tables of contents and canonical Markdown (v0.8)

`toc` reads the headings of a Markdown or HTML document — never a
different conversion, so the matrix is untouched — and renders them as
a nested Markdown list, nested `<ul>`/`<li>` markup, or a JSON array of
`{level, text, anchor}`.

Anchors are GitHub's: lowercase, punctuation dropped, whitespace runs
collapsed to `-`, and repeats numbered `-1`, `-2`… `{"heading_ids":true}`
makes Markdown→HTML write exactly those anchors onto the headings, so a
generated table of contents links to ids the conversion actually
produced. HTML input keeps an `id` the document already carries.

`normalize` re-renders Markdown structurally: one blank line between
blocks, `#` headings, `-` bullets, sequential ordered numbers, GFM
tables with their alignment row, `> ` quotes, `---` rules, and link and
footnote definitions collected at the end. Inline prose keeps its words
and line breaks — those are editorial — but its markup is canonical:
emphasis in the chosen marker, code spans without surplus backticks.
Running it twice changes nothing. `markdown→markdown` in the matrix stays
a byte-faithful identity; normalization is opt-in.

The style layer is opt-in too, and off means "what GitHub writes":
`--emphasis asterisk|underscore` picks the emphasis marker and
`--fence backtick|tilde` the code fence (fences grow automatically when
the code contains them, so a block can never close early).

## Streaming large files

`dc_convert_file_streaming` (plus the MCP `stream` flag and CLI
`--stream`) converts without holding the whole document:

- identity copies of the byte-faithful formats are one 64 KiB copy;
- `text` streams one paragraph at a time;
- CSV and TSV stream one RFC 4180 **record** at a time — records are
  split quote-aware and then parsed by the same `parse_csv`/`parse_tsv`
  the whole-file path uses, so a streamed record cannot disagree;
- Markdown streams one **block** at a time and watches for the two
  constructs that are document-wide (reference link definitions and
  footnotes, plus leading YAML front matter): if one appears it redoes
  the conversion from the whole file, so the output is identical either
  way and only the memory profile differs.

Everything else parses the whole document, because partial parsing would
be a lie rather than an optimisation.

## The matrix (v0.9)

markdown→html, markdown→text, markdown→csv and markdown→tsv (a document
that is one table), html→markdown, html→text, html→csv, html→tsv,
html→json (the first `<table>`), text→html, text→markdown, csv→json,
tsv→json, json→csv, json→tsv, csv↔tsv, csv→markdown (table),
tsv→markdown (table), csv→html (table), tsv→html (table), csv→text,
tsv→text, json→text/html/markdown (pretty-printed, fenced for
Markdown), plus identity conversions that validate and normalize (JSON
pretty-prints, CSV and TSV re-quote canonically).

**TSV mirrors CSV everywhere**: wherever CSV is a source or a target,
TSV is too — same rows, tab as the delimiter.

**Document syntaxes** are read into Markdown and rendered by the
Markdown renderers, so `rst → html` and `rst → markdown` can never
disagree: `rst → markdown|html|text` and `asciidoc → markdown|html|text`.
**OPML** goes both ways — `opml → markdown|html|text` (an outline as
nested bullets, `_note` kept as the item's text) plus `markdown → opml`
and `html → opml` (a document's headings as the outline). 37 pairs in
all.

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
  rules, GFM pipe tables with alignment, and **footnotes**
  (`[^1]` + `[^1]: note` definitions, numbered by first reference and
  collected into a `<section class="footnotes">` block; the text
  flavor numbers them too). **Raw HTML in Markdown is escaped**, so
  output is safe to embed by default.
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
  `colspan`/`rowspan` are not expanded), and **html→json** turns that
  same table into an array of objects. A page with no table is
  `DC_ERR_CORRUPT`, not an empty file.
- **CSV / TSV**: RFC 4180 quoting in both directions, the delimiter
  (`,` or tab) being the only difference — embedded separators, quotes
  and newlines stay inside the cell, so TSV stays lossless even when a
  value itself contains a tab. CSV/TSV→JSON infers numbers, booleans
  and null — with one deliberate exception: leading-zero values like
  `007` stay strings, because they're codes, not quantities.
- **reStructuredText**: sections by adornment order (underlined and
  overlined), bullet and enumerated lists with indentation nesting,
  literal blocks after `::`, block quotes, transitions, comments and
  directives (dropped with their bodies), field lists, **simple and grid
  tables** (they become GFM tables, header rows and all), and inline
  literals, `*emphasis*`, `**strong**`, backtick links
  (`` `text <url>`_ ``) and roles.
- **AsciiDoc**: `= Title` sections, `*`/`**` and `.`/`..` lists,
  `----`/`....` blocks with an optional `[source,lang]`, example and
  quote blocks, comments, attribute and block-attribute lines
  (dropped), admonitions, `'''` breaks, description lists, and inline
  `*strong*`, `_emphasis_`, `+literal+`, `https://…[text]` and
  `<<xref,text>>`. `|===` tables become GFM tables: one row per line,
  first row header.
- **OPML**: nested `<outline text="..." _note="...">`, read with the
  HTML reader (OPML is XML of the same shape) and written with escaped
  attributes. The outline of a document is its headings.
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
  `tests/mcp.rs`. Tables of contents and normalization are pinned in
  all four layers (module tests, CLI, MCP, ABI), as are the three
  document readers (tables included), the OPML round-trip, the normalize
  style layer and the streaming file paths — each streamed conversion is
  asserted equal to its whole-document reference.
- **External cross-check**: the sample document in
  `tests/fixtures/sample.md` was rendered by **marked** (a mainstream
  Markdown engine) and by DocConvert — the outputs are structurally
  identical (verified locally when the fixture was added, and the
  test pins our structure so regressions surface in CI).
- Gate (WinDevKit `tools/gate.sh`): fmt, clippy (host + Windows
  target) with `-D warnings`, full suite — green. Pure logic, no
  Win32 calls.

## Scope limits (v0.9)

Markdown: no HTML pass-through; link titles reach the HTML output (and
come back through html→markdown) but are dropped from the plain-text
flavor, which keeps the label and URL only. markdown→csv/tsv insist on
a document that is one table (plus at most one heading) — prose is an
error, not a guess. Footnotes round-trip through the
`<section class="footnotes">` block DocConvert emits; a page that
numbers footnotes some other way still converts, just without the
`[^n]` markers.
HTML: expects roughly well-formed input (no tag-soup repair
heuristics); html→csv uses the first table only and ignores spans.
CSV→JSON always uses the first row as headers. `toc` understands ATX
and setext headings in Markdown and `<h1>`–`<h6>` in HTML — a document
with no headings yields an empty table of contents, and a source that
carries no headings (CSV, JSON, text) is `NOT_SUPPORTED`. `normalize`
covers block structure, canonical inline markup (emphasis in the chosen
marker, code spans without surplus backticks or padding) and the fence
character; line breaks inside a paragraph are preserved because they are
editorial. rST and AsciiDoc are subsets — their table support covers the
single-line-cell forms, not spans or multi-line cells. Streaming covers
identity copies, text, CSV/TSV records and Markdown blocks; every other
pair needs the whole document to be correct. See ROADMAP.md.

## License

MIT © 2026 Sean Shaffer.

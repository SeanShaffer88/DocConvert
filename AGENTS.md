# DocConvert — agent notes

- Zero dependencies by design: json.rs, csv.rs, markdown.rs, html.rs,
  and frontmatter.rs are all hand-written. Verification anchors: the
  unit suites per module, the marked cross-check fixture
  (tests/fixtures/sample.md + tests/interop.rs), the reference-link /
  front-matter fixture (tests/fixtures/release-notes.md), the file API
  tests (tests/file_io.rs), and the matrix test in convert.rs.
- `convert::MATRIX` is the single source of truth for supported
  pairs; `dc_matrix_json()` is generated from it and `matrix_is_exact`
  fails loudly if dispatch and the table disagree (unsupported pairs
  are NOT_SUPPORTED, never silently degraded). Add a pair in one place
  only: `MATRIX` plus the dispatch arm.
- CSV→JSON inference rule is deliberate and documented: numbers,
  booleans, empty→null; leading-zero values stay strings. Don't
  "fix" 007 into 7. Front matter scalars follow the same rule.
- Markdown output safety: raw HTML is escaped on the md→html path.
  Weakening that changes the library's security posture — don't do
  it without a new explicit opt-in flag.
- Numbers in JSON are stored as source text (precision) — keep it.
- Front matter: stripped by default on md→html / md→text, renderable
  as a table with `{"front_matter":"table"}`, and always readable via
  `dc_front_matter_json`. Malformed front matter stays in the
  document — never dropped silently. Unknown options are
  INVALID_ARG, not ignored.
- Reference link definitions are collected before block parsing
  (skipping fenced code) and normalized with `normalize_label`
  (trim + collapse whitespace + lowercase). Labels are looked up at
  inline-render time, so `Refs` is threaded through every renderer.
- Byte offsets in the Markdown block parser must go through
  `line_indent` / `tail_from` — slicing a line at a raw byte column
  can panic mid-UTF-8.
- Gate: WinDevKit tools/gate.sh (fmt + clippy -D warnings BOTH
  targets + FULL tests). This crate's match-heavy code attracts
  collapsible-match lints; prefer guards from the start.

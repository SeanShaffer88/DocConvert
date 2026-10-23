# DocConvert — agent notes

- Zero dependencies by design: json.rs, csv.rs, markdown.rs, and
  html.rs are all hand-written. Verification anchors: the unit
  suites per module, the marked cross-check fixture
  (tests/fixtures/sample.md + tests/interop.rs), and the conversion
  matrix test in convert.rs.
- The dc_matrix_json() list and convert.rs's dispatch must stay in
  sync — the matrix test fails loudly when a pair is added to one
  and not the other (unsupported pairs are NOT_SUPPORTED, never
  silently degraded).
- CSV→JSON inference rule is deliberate and documented: numbers,
  booleans, empty→null; leading-zero values stay strings. Don't
  "fix" 007 into 7.
- Markdown output safety: raw HTML is escaped on the md→html path.
  Weakening that changes the library's security posture — don't do
  it without a new explicit opt-in flag.
- Numbers in JSON are stored as source text (precision) — keep it.
- Gate: WinDevKit tools/gate.sh (fmt + clippy -D warnings BOTH
  targets + FULL tests). This crate's match-heavy code attracts
  collapsible-match lints; prefer guards from the start.

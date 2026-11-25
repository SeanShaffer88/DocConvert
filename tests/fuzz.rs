//! A seeded sweep over hostile input.
//!
//! The generator builds documents out of the fragments that have hurt these
//! parsers before — pipe runs, fence runs, emphasis runs, HTML brackets,
//! absurd nesting, odd whitespace — and then asserts three things about
//! every one of them:
//!
//! 1. no reader or renderer **panics** (a panic in the DLL takes the host
//!    down, so this is the one bug class a caller cannot work around);
//! 2. `normalize` is **idempotent** — normalizing its own output changes
//!    nothing, and in particular does not grow the document;
//! 3. anything that claims to write **JSON** can be parsed back.
//!
//! Everything is seeded: a failure message carries the seed, the round and
//! the exact input, so it reproduces with no other state. The default sweep
//! is small enough for CI; `cargo test --release -- --ignored` runs the long
//! one, which is what to reach for after a parser change.

use std::panic::catch_unwind;

use docconvert::convert::convert;
use docconvert::json::parse_json;
use docconvert::markdown::normalize;

/// xorshift64*: deterministic, dependency-free, and good enough to shuffle
/// fragments. The seed is always odd so the state can never be zero.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }

    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// Constructs that carry structural meaning in at least one of the formats.
const FRAGMENTS: &[&str] = &[
    "|",
    "||",
    "| --- |",
    "|:--|",
    "---",
    "----",
    "===",
    "+---+",
    "|===",
    "#",
    "##",
    "######",
    ">",
    "> >",
    "- ",
    "* ",
    "1. ",
    "2) ",
    "  - ",
    "*",
    "**",
    "***",
    "_",
    "__",
    "___",
    "`",
    "``",
    "```",
    "~~~",
    "[",
    "]",
    "(",
    ")",
    "![",
    "[^1]",
    "[^1]: note",
    "[a]: https://e.com",
    "<",
    ">",
    "<div>",
    "</div>",
    "<table>",
    "<tr>",
    "<td>",
    "</td>",
    "<p>",
    "<!--",
    "-->",
    "<script>",
    "</script>",
    "&amp;",
    "&",
    "&#x1F600;",
    "{",
    "}",
    "\"",
    "'",
    "\\",
    "\\\\",
    "$",
    "%",
    "^",
    "=",
    ":",
    "::",
    "a",
    "b",
    "é",
    "😀",
    "\u{3000}",
    "\u{a0}",
    " ",
    "\t",
    "\r\n",
    "\n",
    "\n\n",
];

/// One input: a run of random fragments, and half the time one construct
/// blown up to a depth no human would write.
fn document(rng: &mut Rng) -> String {
    let mut out = String::new();
    for _ in 0..1 + rng.below(48) {
        out.push_str(rng.pick(FRAGMENTS));
    }
    if rng.below(2) == 0 {
        let unit = rng.pick(&["<div>", "<span>", "> ", "- ", "  ", "[", "[[", "*", "|==="]);
        for _ in 0..1 + rng.below(4096) {
            out.push_str(unit);
        }
        out.push_str(rng.pick(&["x", "deep", ""]));
    }
    out
}

/// The pairs each input goes through. Every reader, plus the writers that
/// produce JSON (which must be parseable) and OPML.
const PAIRS: &[(&str, &str)] = &[
    ("markdown", "html"),
    ("markdown", "csv"),
    ("markdown", "json"),
    ("markdown", "opml"),
    ("markdown", "markdown"),
    ("html", "markdown"),
    ("html", "csv"),
    ("html", "json"),
    ("html", "opml"),
    ("rst", "markdown"),
    ("rst", "html"),
    ("asciidoc", "markdown"),
    ("asciidoc", "html"),
];

/// Whether normalizing the document twice changes it. Panics are treated as
/// "not idempotent" so the shrinker can work on them too.
fn idempotent(input: &str) -> bool {
    let once = match catch_unwind(|| normalize(input)) {
        Ok(Ok(text)) => text,
        Ok(Err(_)) | Err(_) => return true,
    };
    match catch_unwind(|| normalize(&once)) {
        Ok(Ok(again)) => again == once,
        Ok(Err(_)) | Err(_) => false,
    }
}

/// Greedily delete characters while `fails` still holds, so a report carries
/// the smallest input that shows the bug instead of fifty random fragments.
fn shrink(input: &str, fails: &dyn Fn(&str) -> bool) -> String {
    let mut chars: Vec<char> = input.chars().collect();
    let mut size = (chars.len() / 2).max(1);
    while size > 0 {
        let mut index = 0;
        while index + size <= chars.len() {
            let mut candidate = chars.clone();
            candidate.drain(index..index + size);
            let text: String = candidate.iter().collect();
            if fails(&text) {
                chars = candidate;
            } else {
                index += size;
            }
        }
        size /= 2;
    }
    chars.into_iter().collect()
}

fn sweep(seed: u64, rounds: usize) {
    let mut rng = Rng::new(seed);
    for round in 0..rounds {
        let input = document(&mut rng);
        let label = format!("seed {seed} round {round}: {input:?}");

        if !idempotent(&input) {
            let minimal = shrink(&input, &|text: &str| !idempotent(text));
            let once = normalize(&minimal).expect("normalize");
            let twice = normalize(&once).expect("normalize twice");
            panic!(
                "normalize is not idempotent for {label}\n\
                 smallest failing input: {minimal:?}\n  once:  {once:?}\n  twice: {twice:?}"
            );
        }

        for &(from, to) in PAIRS {
            let converted = catch_unwind(|| convert(&input, from, to))
                .unwrap_or_else(|_| panic!("{from} → {to} panicked on {label}"));
            if let Ok(text) = converted {
                if to == "json" {
                    assert!(
                        parse_json(&text).is_ok(),
                        "{from} → json wrote unparseable JSON for {label}: {text:?}"
                    );
                }
            }
        }
    }
}

/// Small enough for every CI run: four seeds, a hundred documents each.
#[test]
fn seeded_sweep_is_quiet() {
    for seed in 1..=4u64 {
        sweep(seed, 100);
    }
}

/// The same generator, much longer. `cargo test --release -- --ignored`.
#[test]
#[ignore = "the long sweep; run with --ignored after a parser change"]
fn seeded_sweep_long() {
    for seed in 1..=32u64 {
        sweep(seed, 2_000);
    }
}

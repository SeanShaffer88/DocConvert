//! A seeded sweep over hostile input.
//!
//! The generator builds documents out of the fragments that have hurt these
//! parsers before — pipe runs, fence runs, emphasis runs, HTML brackets,
//! absurd nesting, odd whitespace — and then asserts three things about
//! every one of them:
//!
//! 1. no reader or renderer **panics**, and none of them **hangs** (a panic
//!    in the DLL takes the host down and a loop takes it round and round, so
//!    these are the two bug classes a caller cannot work around);
//! 2. `normalize` is **idempotent** — normalizing its own output changes
//!    nothing, and in particular does not grow the document;
//! 3. anything that claims to write **JSON** can be parsed back.
//!
//! Everything is seeded: a failure message carries the seed, the round and
//! the input, so it reproduces with no other state. A failing idempotence
//! check is shrunk first, so the message normally carries four or five
//! characters rather than fifty random fragments. The default sweep is small
//! enough for CI; `cargo test --release -- --ignored` runs the long one,
//! which is what to reach for after a parser change.

use std::panic::catch_unwind;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

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
        // Past every cap in the crate (256 for HTML, 128 for JSON, 48 for
        // Markdown blocks) but small enough that a thousand of these still
        // finish in seconds on a CI runner.
        let unit = rng.pick(&["<div>", "<span>", "> ", "- ", "  ", "[", "[[", "*", "|==="]);
        for _ in 0..1 + rng.below(300) {
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

/// How long one conversion may take before the sweep calls it a hang.
const BUDGET: Duration = Duration::from_secs(10);

/// Run `work` on another thread, so a loop that never ends is *reported*
/// instead of leaving the suite (and the CI job) waiting for a cancel.
fn within<T: Send + 'static>(label: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    match rx.recv_timeout(BUDGET) {
        Ok(value) => value,
        Err(RecvTimeoutError::Timeout) => {
            panic!("a conversion did not finish within {:?}\n{label}", BUDGET)
        }
        Err(RecvTimeoutError::Disconnected) => panic!("a conversion panicked\n{label}"),
    }
}

/// The message of a caught panic, when it has one.
fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    if let Some(text) = payload.downcast_ref::<&str>() {
        return text.to_string();
    }
    "a conversion panicked without a message".to_string()
}

/// Greedily delete characters while `fails` still holds, so a report carries
/// the smallest input that shows the bug instead of fifty random fragments.
///
/// Bounded on purpose: every attempt costs two conversions, so only small
/// inputs are shrunk and the number of attempts is capped. A report from the
/// long sweep is worth more than a slow shrink of a generated 20 kB document
/// (and an unbounded shrinker turns one failure into a hung CI job).
fn shrink(input: &str, fails: &dyn Fn(&str) -> bool) -> String {
    const MAX_CHARS: usize = 2_000;
    const MAX_ATTEMPTS: usize = 3_000;
    let mut chars: Vec<char> = input.chars().collect();
    if chars.len() > MAX_CHARS {
        return input.to_string();
    }
    let mut attempts = 0usize;
    let mut size = (chars.len() / 2).max(1);
    while size > 0 && attempts < MAX_ATTEMPTS {
        let mut index = 0;
        while index + size <= chars.len() && attempts < MAX_ATTEMPTS {
            attempts += 1;
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

/// Whether normalizing the document twice changes it — the predicate the
/// shrinker works on. (The sweep itself runs each pass under `within`.)
fn changes_when_normalized_twice(input: &str) -> bool {
    let once = match normalize(input) {
        Ok(text) => text,
        Err(_) => return false,
    };
    match normalize(&once) {
        Ok(again) => again != once,
        Err(_) => true,
    }
}

fn sweep(seed: u64, rounds: usize) {
    let mut rng = Rng::new(seed);
    for round in 0..rounds {
        let input = document(&mut rng);
        let label = format!("seed {seed} round {round}: {input:?}");

        // Liveness and stability, in that order: a hang is reported with the
        // input that caused it, before anything tries to shrink it.
        let source = input.clone();
        let once = match catch_unwind(move || within(&label, move || normalize(&source))) {
            Ok(Ok(text)) => text,
            Ok(Err(error)) => panic!("normalize failed on {label}: {error:?}"),
            Err(payload) => panic!("{}", panic_text(payload)),
        };
        let second = once.clone();
        let twice = match catch_unwind(move || within(&label, move || normalize(&second))) {
            Ok(Ok(text)) => text,
            Ok(Err(error)) => panic!("normalize failed on its own output for {label}: {error:?}"),
            Err(payload) => panic!("{}", panic_text(payload)),
        };
        if twice != once {
            let minimal = shrink(&input, &changes_when_normalized_twice);
            let minimal_once = normalize(&minimal).unwrap_or_default();
            let minimal_twice = normalize(&minimal_once).unwrap_or_default();
            panic!(
                "normalize is not idempotent.\nsmallest failing input: {minimal:?}\n\
                 once:  {minimal_once:?}\n  twice: {minimal_twice:?}\n{label}"
            );
        }

        for &(from, to) in PAIRS {
            let source = input.clone();
            let pair_label = format!("{from} → {to}\n{label}");
            let converted =
                match catch_unwind(move || within(&pair_label, move || convert(&source, from, to)))
                {
                    Ok(result) => result,
                    Err(payload) => panic!("{}", panic_text(payload)),
                };
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

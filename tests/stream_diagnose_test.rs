//! What the diagnosing pass of a stream must see (ADR 27 §3, *Errors*): every
//! failure the parse in memory records on its way to an error - also one an
//! item recorded several items back, where an alternative it tried and
//! abandoned looked past the items after it.

use std::io::Read;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Far {
        // `aaa!` is tried at every `a` and abandoned for one `a`: the attempt
        // at the first `a` fails three items on.
        #[stream]
        pub DOC -> usize = n:fold(ITEM, || 0usize, |n: usize, _i: ()| n + 1) eof -> { n }
        ITEM -> () = "a" "a" "a" "!" -> { () } | "a" -> { () }

        // The same with a lookahead that reaches far: `peek` looks past the
        // `,`-separated items after it for a `;`.
        #[stream]
        pub LIST -> usize = n:fold(ENTRY, || 0usize, |n: usize, _i: ()| n + 1) eof -> { n }
        ENTRY -> () = "x" not(("," "x" "," "x" "," "x" "?")) ","? -> { () }
    }
}

grammar! {
    // An option and a repetition whose attempt reads two tokens before it
    // fails: `x` with `yz` after it is one item, `x y` is an `x` and a `y`.
    grammar Tails {
        #[stream]
        pub OPT -> usize = n:fold(ITEM, || 0usize, |n: usize, _i: ()| n + 1) eof -> { n }
        ITEM -> () = "x" ("y" "z" "z")? -> { () } | "y" -> { () } | "z" -> { () }

        #[stream]
        pub REP -> usize = n:fold(RITEM, || 0usize, |n: usize, _i: ()| n + 1) eof -> { n }
        RITEM -> () = "x" ("y" "z" "z")* -> { () } | "y" -> { () } | "z" -> { () }
    }
}

grammar! {
    // Built-in tokens under `?`, which the analysis counts as failing where
    // they begin - an `u8` that overflows, a float, a word - so that this
    // grammar is generated without reach tracking: if one of them recorded a
    // failure further on, the items after it would not see it.
    grammar Atoms {
        #[stream]
        pub DOC -> usize = n:fold(ITEM, || 0usize, |n: usize, _i: ()| n + 1) eof -> { n }
        ITEM -> () =
            "n" u8? -> { () }
          | "f" f64? -> { () }
          | "w" raw_ident? -> { () }
          | "3" -> { () } | "0" -> { () } | "9" -> { () } | "." -> { () } | "e" -> { () }
    }
}

struct Trickle<'d>(&'d [u8], usize);

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.1.min(buf.len()).min(self.0.len());
        buf[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

fn in_memory<'a>(
    p: &mut impl Parser<ParseInput<'a, ()>, usize, ParseError>,
    input: &'a str,
    diagnose: Diagnose,
) -> Result<usize, ParseError> {
    p.parse_next(&mut ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::<()> {
            diagnose,
            ..Default::default()
        },
    })
}

fn agree(what: &str, mem: &Result<usize, ParseError>, got: &Result<usize, ParseError>) {
    match (mem, got) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "{what}"),
        (Err(a), Err(b)) => {
            assert_eq!(
                a.offset, b.offset,
                "{what}: offsets\n mem: {a:?}\n got: {b:?}"
            );
            assert_eq!(a.headline(), b.headline(), "{what}: messages");
        }
        _ => panic!("{what}:\n mem: {mem:?}\n got: {got:?}"),
    }
}

/// Small blocks: every read is a refill, and a test of many inputs is quick.
const SMALL: Reading = Reading {
    block: 16,
    max_frame: 256,
};

const MODES: [Diagnose; 3] = [Diagnose::Replay, Diagnose::ReplayInPlace, Diagnose::Eager];

fn check(
    inputs: &[&str],
    mem: impl Fn(&str, Diagnose) -> Result<usize, ParseError>,
    read: impl Fn(&str, usize, Diagnose) -> Result<usize, ParseError>,
) {
    for input in inputs {
        for mode in MODES {
            let m = mem(input, mode);
            for step in [1, 2, 3, 1 << 20] {
                agree(
                    &format!("{input:?} {mode:?} step {step}"),
                    &m,
                    &read(input, step, mode),
                );
            }
        }
    }
}

#[test]
fn a_failure_recorded_several_items_back_is_seen() {
    check(
        &["aaab", "aaaab", "aab", "ab", "aaaaaaab", "aaa!aab"],
        |s, mode| in_memory(&mut Far::parse_DOC(), s, mode),
        |s, step, mode| {
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            Far::parse_DOC_reader(Trickle(s.as_bytes(), step), &mut ctx, SMALL)
        },
    );
}

#[test]
fn a_lookahead_that_reaches_past_items_is_seen() {
    check(
        &["x,x,x,x;", "x,x,x,x,x,x;", "x,x,x,x?", "x,x;"],
        |s, mode| in_memory(&mut Far::parse_LIST(), s, mode),
        |s, step, mode| {
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            Far::parse_LIST_reader(Trickle(s.as_bytes(), step), &mut ctx, SMALL)
        },
    );
}

/// Every input up to `len` over `alphabet`.
fn all(alphabet: &[&str], len: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut last = vec![String::new()];
    for _ in 0..len {
        last = last
            .iter()
            .flat_map(|p| alphabet.iter().map(move |a| format!("{p}{a}")))
            .collect();
        out.extend(last.iter().cloned());
    }
    out
}

#[test]
fn every_short_input_agrees() {
    let doc = all(&["a", "!", "b"], 7);
    let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
    check(
        &doc,
        |s, mode| in_memory(&mut Far::parse_DOC(), s, mode),
        |s, step, mode| {
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            Far::parse_DOC_reader(Trickle(s.as_bytes(), step), &mut ctx, SMALL)
        },
    );
    let list = all(&["x", ",", "?", ";"], 6);
    let list: Vec<&str> = list.iter().map(String::as_str).collect();
    check(
        &list,
        |s, mode| in_memory(&mut Far::parse_LIST(), s, mode),
        |s, step, mode| {
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            Far::parse_LIST_reader(Trickle(s.as_bytes(), step), &mut ctx, SMALL)
        },
    );
}

#[test]
fn items_kept_for_the_diagnosis_are_dropped_once_passed() {
    // A long input whose items all reach past their end: the buffer must not
    // keep them all, and the answer stays the parse in memory's.
    let input = format!("{}b", "a".repeat(200_000));
    let mem = in_memory(&mut Far::parse_DOC(), &input, Diagnose::Replay);
    let mut ctx = ParseContext::<()>::default();
    let got = Far::parse_DOC_reader(
        Trickle(input.as_bytes(), 1000),
        &mut ctx,
        Reading {
            block: 4096,
            max_frame: 4096,
        },
    );
    agree("long", &mem, &got);
}

#[test]
fn built_in_tokens_fail_where_they_begin() {
    let doc = all(&["n", "f", "w", "3", "0", "9", ".", "e"], 5);
    let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
    check(
        &doc,
        |s, mode| in_memory(&mut Atoms::parse_DOC(), s, mode),
        |s, step, mode| {
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            Atoms::parse_DOC_reader(Trickle(s.as_bytes(), step), &mut ctx, SMALL)
        },
    );
}

#[test]
fn an_option_or_a_repetition_that_reads_ahead_is_seen() {
    let inputs = all(&["x", "y", "z", "w"], 6);
    let inputs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    check(
        &inputs,
        |s, mode| in_memory(&mut Tails::parse_OPT(), s, mode),
        |s, step, mode| {
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            Tails::parse_OPT_reader(Trickle(s.as_bytes(), step), &mut ctx, SMALL)
        },
    );
    check(
        &inputs,
        |s, mode| in_memory(&mut Tails::parse_REP(), s, mode),
        |s, step, mode| {
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            Tails::parse_REP_reader(Trickle(s.as_bytes(), step), &mut ctx, SMALL)
        },
    );
}

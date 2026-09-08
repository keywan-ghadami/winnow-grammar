//! Lazy diagnostics - the promises of ADR 17 (`docs/adr/adr17-lazy-diagnostics.md`):
//! a parse is first run without any error bookkeeping, and only a failure is
//! parsed again with the full engine of ADR 15. In case of conflict, the ADR
//! wins.
//!
//! What is checked: the replay reports exactly what a direct diagnosis
//! reports (message and every field); `Diagnose::Off` gives the verdict and
//! nothing else; actions run as the contract says; and on a `par_fold` rule
//! the replay parses one item, not the input.

use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use winnow::error::{EmptyError, ErrMode};
use winnow::prelude::*;
use winnow::stream::{LocatingSlice, Stateful};
use winnow_grammar::rt::{self, Parallelism};
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseError, ParseInput};

/// A hand-written parser plugged into the grammar below: it returns
/// `ParseError` whatever pass calls it (ADR 17, side-effect contract).
fn word<'a, S: Clone + std::fmt::Debug>(i: &mut ParseInput<'a, S>) -> Result<&'a str, ParseError> {
    winnow::token::take_while(1.., |c: char| c.is_alphabetic()).parse_next(i)
}

grammar! {
    grammar Diag {
        // The grammar of `tests/diagnostics.rs`, so that every kind of error
        // ADR 15 produces goes through the replay here: aggregation with the
        // found token, a discarded backtrack (`arg*`), labels, `fail`, and a
        // bounded repetition, a cut and a plugged-in parser on top.
        pub decl -> String = "fn" name:raw_ident "(" args:arg* ")" ";" -> { format!("{name}({})", args.join(",")) }
        arg -> String = n:raw_ident ":" t:ty ","? -> { format!("{n}:{t}") }
        ty -> String = t:raw_ident -> { t.to_string() } | "&" t:raw_ident -> { format!("&{t}") }

        pub assign -> String = "let" v:value ";" -> { v }
        value -> String = n:u32 # "number" -> { n.to_string() } | s:string # "string" -> { s.to_string() }

        pub guarded -> u32 = "a" fail("custom failure here") -> { 0 } | "a" "b" "c" -> { 1 }

        pub bounded -> usize = "[" xs:raw_ident{2,3} "]" -> { xs.len() }

        pub committed -> u32 = "if" => "(" n:u32 ")" -> { n } | "if" "x" -> { 0 }

        pub plugged -> String = "@" w:super::word ";" -> { w.to_string() }
    }
}

fn context<S: Default>(mode: Diagnose) -> ParseContext<S> {
    ParseContext::<S> {
        diagnose: mode,
        ..Default::default()
    }
}

fn run<'a, O, P>(mode: Diagnose, mut parser: P, input: &'a str) -> Result<O, ParseError>
where
    P: Parser<ParseInput<'a, ()>, O, ParseError>,
{
    let mut stream = Stateful {
        input: LocatingSlice::new(input),
        state: context::<()>(mode),
    };
    parser.parse_next(&mut stream)
}

/// The replay must be indistinguishable from a direct diagnosis: same value
/// on success, and on failure the same rendering and the same fields -
/// `offset`, `expected`, `message`, `found`, `rule_stack`, `priority`.
fn assert_same_answer<'a, O, P>(make: impl Fn() -> P, inputs: &[&'a str])
where
    O: PartialEq + std::fmt::Debug,
    P: Parser<ParseInput<'a, ()>, O, ParseError>,
{
    for input in inputs {
        let direct = run(Diagnose::Eager, make(), input);
        for mode in [Diagnose::Replay, Diagnose::ReplayInPlace] {
            let replayed = run(mode, make(), input);
            match (&direct, &replayed) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "{mode:?} on {input:?}"),
                (Err(a), Err(b)) => {
                    assert_eq!(a.render(input), b.render(input), "{mode:?} on {input:?}");
                    // `ParseError` compares its whole content.
                    assert_eq!(a, b, "{mode:?} on {input:?}");
                    assert!(!b.is_undiagnosed());
                }
                _ => panic!("{mode:?} on {input:?}: direct {direct:?}, replayed {replayed:?}"),
            }
        }
    }
}

const DECLS: &[&str] = &[
    "fn f(a: u32);",
    "fn f(a: &u32, b: u32);",
    "fn f();",
    "fn f(a: );",
    "fn f(\n    a: );",
    "fn f(a: u32) extra",
    "fn f(a: u32",
    "",
    "fn",
];

#[test]
fn replay_equals_direct_diagnosis() {
    assert_same_answer(Diag::parse_decl, DECLS);
    assert_same_answer(
        Diag::parse_assign,
        &["let 1;", "let \"s\";", "let x;", "let 1", "let"],
    );
    assert_same_answer(Diag::parse_guarded, &["abc", "a", "ab", "b"]);
    assert_same_answer(
        Diag::parse_bounded,
        &["[a b]", "[a b c]", "[a]", "[a b c d]", "[]", "["],
    );
    assert_same_answer(
        Diag::parse_committed,
        &["if (1)", "if x", "if (x)", "if (", "if y"],
    );
    assert_same_answer(Diag::parse_plugged, &["@abc;", "@abc", "@1;", "@"]);
}

/// The one thing the fast pass cannot explain is a leftover: the reason it
/// stopped was recorded, and only the diagnosing pass records. So a leftover
/// is replayed too, and names the reason as before.
#[test]
fn a_leftover_is_replayed_and_names_the_reason() {
    let e = run(Diagnose::Replay, Diag::parse_decl(), "fn f(a: u32) extra").unwrap_err();
    let rendered = e.render("fn f(a: u32) extra");
    assert!(
        rendered.starts_with("expected `;`; found unexpected token `extra`"),
        "{rendered}"
    );
}

/// `Off`: the verdict, nothing else. Same accept/reject as every other mode,
/// same value on success, and an error that claims no position.
#[test]
fn off_is_the_verdict_alone() {
    for input in DECLS {
        let direct = run(Diagnose::Eager, Diag::parse_decl(), input);
        let off = run(Diagnose::Off, Diag::parse_decl(), input);
        match (direct, off) {
            (Ok(a), Ok(b)) => assert_eq!(a, b, "{input:?}"),
            (Err(_), Err(e)) => {
                assert!(e.is_undiagnosed(), "{input:?}");
                assert_eq!(e.render(input), "parse failed; diagnostics are off");
                assert!(!e.render(input).contains("line"), "{input:?}");
                assert_eq!(e.to_string(), "parse failed; diagnostics are off");
            }
            (a, b) => panic!("{input:?}: direct {a:?}, off {b:?}"),
        }
    }
}

// -----------------------------------------------------------------------------
// The side-effect contract, at the level of `rt::entry`: what an action that
// mutates `user_state` sees under each mode. (A grammar action cannot do this
// to a generic `S`, so the passes are written by hand here.)
// -----------------------------------------------------------------------------

fn runs_under(mode: Diagnose, fast_outcome: &str) -> u32 {
    let mut stream = Stateful {
        input: LocatingSlice::new("x"),
        state: context::<u32>(mode),
    };
    let outcome = fast_outcome;
    let fast = |i: &mut ParseInput<'_, u32>| -> Result<(), ErrMode<EmptyError>> {
        i.state.user_state += 1;
        match outcome {
            "accepts" => {
                let all = i.eof_offset();
                i.next_slice(all);
                Ok(())
            }
            "leaves input over" => Ok(()),
            _ => Err(ErrMode::Backtrack(EmptyError)),
        }
    };
    let diagnose = |i: &mut ParseInput<'_, u32>| -> Result<(), ErrMode<ParseError>> {
        i.state.user_state += 1;
        Err(ErrMode::Backtrack(ParseError::from_stream(i)))
    };
    let _ = rt::entry(&mut stream, fast, diagnose);
    stream.state.user_state
}

#[test]
fn an_action_runs_once_under_replay_twice_in_place_once_when_eager_or_off() {
    for outcome in ["fails", "leaves input over"] {
        assert_eq!(runs_under(Diagnose::Replay, outcome), 1, "{outcome}");
        assert_eq!(runs_under(Diagnose::ReplayInPlace, outcome), 2, "{outcome}");
        assert_eq!(runs_under(Diagnose::Eager, outcome), 1, "{outcome}");
        assert_eq!(runs_under(Diagnose::Off, outcome), 1, "{outcome}");
    }
    // A fast pass that accepts is the only pass.
    for mode in [Diagnose::Replay, Diagnose::ReplayInPlace, Diagnose::Off] {
        assert_eq!(runs_under(mode, "accepts"), 1, "{mode:?}");
    }
}

// -----------------------------------------------------------------------------
// The framed replay: on a `par_fold` rule the diagnosing pass starts at the
// item the fast pass stopped in. `ATTEMPTS` counts every attempt at an item,
// in both passes - the action of `NAME` runs whether or not `REC` then fails.
// -----------------------------------------------------------------------------

static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

grammar! {
    grammar Lengths {
        NAME -> &'a str = s:until(";" | frame_end) -> { ATTEMPTS.fetch_add(1, Relaxed); s }
        #[frame]
        pub REC -> usize = n:NAME ";" "\n" -> { n.len() }
        pub FILE -> usize =
            s:par_fold(REC, || 0usize, |a: usize, v: usize| a + v, |a: usize, b: usize| a + b)
            -> { s }
    }
}

/// `lines` records, the `bad`-th (1-based) without its `;`.
fn records(lines: usize, bad: Option<usize>) -> String {
    (1..=lines)
        .map(|k| {
            if Some(k) == bad {
                format!("rec{k}\n")
            } else {
                format!("rec{k};\n")
            }
        })
        .collect()
}

/// Runs `parse` and returns its result with the number of item attempts.
fn counting<T>(parse: impl FnOnce() -> T) -> (T, usize) {
    // The tests of this file share the counter; serialise them on it.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ATTEMPTS.store(0, Relaxed);
    let result = parse();
    (result, ATTEMPTS.load(Relaxed))
}

fn sequential(mode: Diagnose, input: &str) -> (Result<usize, ParseError>, usize) {
    counting(|| run(mode, Lengths::parse_FILE(), input))
}

fn in_pieces(mode: Diagnose, how: Parallelism, input: &str) -> (Result<usize, ParseError>, usize) {
    counting(|| Lengths::parse_FILE_pieces(input, || context::<()>(mode), how))
}

#[test]
fn the_framed_replay_parses_one_item_and_reports_the_same_error() {
    for (lines, bad) in [(50, 17), (50, 1), (50, 50), (3, 2)] {
        let input = records(lines, Some(bad));
        let (direct, attempts_direct) = sequential(Diagnose::Eager, &input);
        let direct = direct.expect_err("a record without `;`");
        // The direct diagnosis attempts every record up to the bad one.
        assert_eq!(attempts_direct, bad, "lines {lines}, bad {bad}");
        let rendered = direct.render(&input);
        assert!(rendered.contains(&format!("line {bad},")), "{rendered}");
        assert!(rendered.contains(&format!("in item {bad}")), "{rendered}");

        for mode in [Diagnose::Replay, Diagnose::ReplayInPlace] {
            let (replayed, attempts) = sequential(mode, &input);
            let replayed = replayed.expect_err("a record without `;`");
            assert_eq!(replayed.render(&input), rendered, "{mode:?}");
            assert_eq!(replayed, direct, "{mode:?}");
            // The fast pass made the same attempts; the replay made ONE.
            assert_eq!(
                attempts,
                attempts_direct + 1,
                "{mode:?}: lines {lines}, bad {bad}"
            );
        }
    }
}

#[test]
fn a_good_file_costs_the_fast_pass_alone() {
    let input = records(40, None);
    let (direct, attempts_direct) = sequential(Diagnose::Eager, &input);
    let (fast, attempts_fast) = sequential(Diagnose::Replay, &input);
    assert_eq!(fast.unwrap(), direct.unwrap());
    // 40 records and one attempt at a 41st, in both.
    assert_eq!(attempts_fast, attempts_direct);
    assert_eq!(attempts_fast, 41);
}

#[test]
fn a_leftover_after_the_fold_is_replayed_from_the_item_that_stopped_it() {
    // Record 3 is empty: `NAME` matches nothing, `;` is missing, the fold
    // stops, and a newline is left over. The reason is the recorded error.
    let input = "a;\nbb;\n\nccc;\n";
    let (direct, attempts_direct) = sequential(Diagnose::Eager, input);
    let direct = direct.expect_err("an empty record");
    let (replayed, attempts) = sequential(Diagnose::Replay, input);
    let replayed = replayed.expect_err("an empty record");
    assert_eq!(replayed.render(input), direct.render(input));
    assert_eq!(replayed, direct);
    assert_eq!(attempts, attempts_direct + 1);
    assert!(
        direct.render(input).contains("in item 3"),
        "{}",
        direct.render(input)
    );
}

#[test]
fn every_piece_replays_from_its_failing_item() {
    let input = records(60, Some(41));
    for how in [
        Parallelism::Off,
        Parallelism::Pieces(3),
        Parallelism::Pieces(6),
        Parallelism::Pieces(60),
    ] {
        let (direct, attempts_direct) = in_pieces(Diagnose::Eager, how, &input);
        let direct = direct.expect_err("record 41 without `;`");
        assert!(
            direct.render(&input).contains("line 41,"),
            "{how:?}: {}",
            direct.render(&input)
        );

        for mode in [Diagnose::Replay, Diagnose::ReplayInPlace] {
            let (replayed, attempts) = in_pieces(mode, how, &input);
            let replayed = replayed.expect_err("record 41 without `;`");
            assert_eq!(
                replayed.render(&input),
                direct.render(&input),
                "{how:?} {mode:?}"
            );
            assert_eq!(replayed, direct, "{how:?} {mode:?}");
            assert_eq!(attempts, attempts_direct + 1, "{how:?} {mode:?}");
        }

        let (off, _) = in_pieces(Diagnose::Off, how, &input);
        let off = off.expect_err("record 41 without `;`");
        assert!(off.is_undiagnosed(), "{how:?}");
        assert_eq!(off.render(&input), "parse failed; diagnostics are off");
    }

    // And a good file: pieces agree with the sequential parse, fast.
    let input = records(60, None);
    for how in [Parallelism::Off, Parallelism::Pieces(7), Parallelism::Auto] {
        let (v, _) = in_pieces(Diagnose::Replay, how, &input);
        assert_eq!(
            v.unwrap(),
            sequential(Diagnose::Eager, &input).0.unwrap(),
            "{how:?}"
        );
    }
}

/// A fresh context starts every mode from the same place; the default is
/// the replay with a snapshot.
#[test]
fn the_default_mode_is_replay() {
    assert_eq!(ParseContext::<()>::default().diagnose, Diagnose::Replay);
    assert_eq!(Diagnose::default(), Diagnose::Replay);
}

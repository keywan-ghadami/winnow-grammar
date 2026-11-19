//! `ParseContext::emit` (ADR 28): an action anywhere in a parse run by
//! `parse_<rule>_reader_each` hands a value of the item type to `each`,
//! between the items, in the order the parse takes them.
//!
//! What is promised, and checked here under every `Diagnose` mode and read a
//! byte at a time as well as whole:
//!
//! * a value emitted by an action - of a rule nested in the item, of the
//!   prefix, of the whole rule - reaches `each` exactly once, before the item
//!   whose parse emitted it, also where a refill or a failure makes the
//!   stream run a step again;
//! * a `Break` on an emitted value ends the parse as a `Break` on an item
//!   does: what follows is not read, and is not checked;
//! * outside `_reader_each` an emit goes nowhere and says so: `false`.

use std::cell::Cell;
use std::io::Read;
use std::ops::ControlFlow;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Ev {
        // Emits from the prefix, from a rule two levels inside the item, and
        // from the rule's own action; the items are the entries' keys.
        #[stream(each)]
        pub doc -> usize =
            "[" _t:title ";" n:fold(entry, || 0usize, |n: usize, _e: String| n + 1) "]"
            -> { _state.emit(format!("end {n}")); n }
        title -> String = s:raw_ident -> { _state.emit(format!("title {s}")); s.to_string() }
        entry -> String = k:raw_ident "=" _v:value ","? -> { k.to_string() }
        value -> i64 = n:i64 -> {
            if n < 0 {
                _state.emit(format!("neg {n}"));
            }
            n
        }
    }
}

grammar! {
    grammar Quiet {
        // The same action, but no `each`: in memory and from a reader.
        #[stream]
        pub doc -> (usize, bool) =
            "[" r:fold(entry, || (0usize, false), |(n, e): (usize, bool), x: bool| (n + 1, e || x)) "]"
            -> { r }
        entry -> bool = _k:raw_ident "=" v:i64 ","? -> { _state.emit(format!("{v}")) }
    }
}

grammar! {
    grammar Tail {
        // No suffix: after the fold comes the rule's action, so what the
        // item attempt that ended the fold emitted must not reach `each`.
        #[stream(each)]
        pub doc -> usize =
            n:fold(entry, || 0usize, |n: usize, _e: String| n + 1) -> { n }
        entry -> String = _t:attempt v:i64 ";" -> { v.to_string() }
        // Matches nothing as well, so the attempt at the end emits too.
        attempt -> bool = b:"!"? -> {
            _state.emit("try".to_string());
            b.is_some()
        }
    }
}

grammar! {
    grammar Wrong {
        #[stream(each)]
        pub doc -> usize =
            n:fold(entry, || 0usize, |n: usize, _e: String| n + 1) eof -> { n }
        entry -> String = v:i64 ";" -> { _state.emit(v); v.to_string() }
    }
}

/// At most `step` bytes a read; counts what it handed out.
struct Trickle<'d, 'c> {
    data: &'d [u8],
    step: usize,
    read: &'c Cell<usize>,
}

impl Read for Trickle<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.step.min(buf.len()).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        self.read.set(self.read.get() + n);
        Ok(n)
    }
}

const MODES: [Diagnose; 4] = [
    Diagnose::Off,
    Diagnose::Replay,
    Diagnose::ReplayInPlace,
    Diagnose::Eager,
];
const STEPS: [usize; 3] = [1, 5, 1 << 20];
const SMALL: Reading = Reading {
    block: 16,
    max_frame: 256,
};

fn ctx(diagnose: Diagnose) -> ParseContext<()> {
    ParseContext {
        diagnose,
        ..Default::default()
    }
}

/// `n` entries, every third value negative.
fn entries(n: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| {
            format!(
                "k{i}={}",
                if i % 3 == 0 {
                    -(i as i64) - 1
                } else {
                    i as i64
                }
            )
        })
        .collect();
    format!("[ title ; {} ]", items.join(", "))
}

/// What `each` should see for `entries(n)`.
fn expected(n: usize) -> Vec<String> {
    let mut want = vec!["title title".to_string()];
    for i in 0..n {
        if i % 3 == 0 {
            want.push(format!("neg {}", -(i as i64) - 1));
        }
        want.push(format!("k{i}"));
    }
    want.push(format!("end {n}"));
    want
}

type Answer = Result<ControlFlow<usize, usize>, ParseError>;

fn each_reader(
    input: &str,
    step: usize,
    mode: Diagnose,
    stop: impl Fn(&str) -> bool,
) -> (Answer, Vec<String>, usize) {
    let read = Cell::new(0);
    let mut seen = Vec::new();
    let r = Ev::parse_doc_reader_each(
        Trickle {
            data: input.as_bytes(),
            step,
            read: &read,
        },
        &mut ctx(mode),
        SMALL,
        |v: &String| {
            seen.push(v.clone());
            if stop(v) {
                ControlFlow::Break(seen.len())
            } else {
                ControlFlow::Continue(())
            }
        },
    );
    (r, seen, read.get())
}

#[test]
fn an_emit_from_a_nested_action_reaches_each_once_before_its_item() {
    let input = entries(40);
    for mode in MODES {
        for step in STEPS {
            let (r, seen, _) = each_reader(&input, step, mode, |_| false);
            assert_eq!(seen, expected(40), "{mode:?} step {step}");
            assert_eq!(
                r.unwrap(),
                ControlFlow::Continue(40),
                "{mode:?} step {step}"
            );
        }
    }
}

#[test]
fn a_break_on_an_emitted_value_stops_the_parse_there() {
    let input = entries(100_000);
    for mode in MODES {
        for step in [1, 64] {
            // `neg -4` is emitted inside the fourth entry, before its item.
            let (r, seen, read) = each_reader(&input, step, mode, |v| v == "neg -4");
            assert_eq!(seen, expected(4)[..6], "{mode:?} step {step}");
            assert_eq!(r.unwrap(), ControlFlow::Break(6), "{mode:?} step {step}");
            assert!(
                read <= 256 + 64 + 64,
                "{mode:?} step {step}: read {read} bytes"
            );
        }
    }
}

#[test]
fn what_follows_a_break_on_an_emitted_value_is_never_checked() {
    let mut input = entries(10);
    input.truncate(input.find("k5").unwrap());
    input.push_str("k5 = oops");
    for mode in MODES {
        for step in STEPS {
            let (r, _, _) = each_reader(&input, step, mode, |v| v == "neg -1");
            assert_eq!(r.unwrap(), ControlFlow::Break(2), "{mode:?} step {step}");
        }
    }
}

#[test]
fn a_break_on_the_rules_own_emit_comes_after_the_last_item() {
    let input = entries(7);
    for mode in MODES {
        for step in STEPS {
            let (r, seen, _) = each_reader(&input, step, mode, |v| v.starts_with("end"));
            assert_eq!(seen, expected(7), "{mode:?} step {step}");
            assert_eq!(
                r.unwrap(),
                ControlFlow::Break(expected(7).len()),
                "{mode:?} step {step}"
            );
        }
    }
}

#[test]
fn what_the_attempt_that_ends_the_fold_emitted_is_dropped() {
    for mode in MODES {
        for step in STEPS {
            let mut seen = Vec::new();
            let r = Tail::parse_doc_reader_each(
                Trickle {
                    data: b"1;!2;",
                    step,
                    read: &Cell::new(0),
                },
                &mut ctx(mode),
                SMALL,
                |v: &String| {
                    seen.push(v.clone());
                    ControlFlow::<usize>::Continue(())
                },
            );
            assert_eq!(seen, ["try", "1", "try", "2"], "{mode:?} step {step}");
            assert_eq!(r.unwrap(), ControlFlow::Continue(2), "{mode:?} step {step}");
        }
    }
}

#[test]
fn a_grammar_without_each_is_unaffected_and_its_emits_go_nowhere() {
    let input = "[ a = 1, b = 2, c = 3 ]";
    for mode in MODES {
        let mut i = ParseInput {
            input: LocatingSlice::new(input),
            state: ctx(mode),
        };
        let r = Quiet::parse_doc().parse_next(&mut i).unwrap();
        assert_eq!(r, (3, false), "{mode:?} in memory");
        for step in STEPS {
            let read = Cell::new(0);
            let r = Quiet::parse_doc_reader(
                Trickle {
                    data: input.as_bytes(),
                    step,
                    read: &read,
                },
                &mut ctx(mode),
                SMALL,
            );
            assert_eq!(r.unwrap(), (3, false), "{mode:?} step {step}");
        }
    }
    // Nor does a context that ran an `each` parse carry its sink on.
    let mut c = ctx(Diagnose::Off);
    let _ = Ev::parse_doc_reader_each(
        Trickle {
            data: entries(2).as_bytes(),
            step: 1 << 20,
            read: &Cell::new(0),
        },
        &mut c,
        SMALL,
        |_: &String| ControlFlow::<usize>::Continue(()),
    );
    assert!(!c.emit("after".to_string()));
}

#[test]
#[should_panic(expected = "takes")]
fn an_emit_of_another_type_than_the_items_panics() {
    let _ = Wrong::parse_doc_reader_each(
        Trickle {
            data: b"1;2;",
            step: 1 << 20,
            read: &Cell::new(0),
        },
        &mut ctx(Diagnose::Off),
        SMALL,
        |_: &String| ControlFlow::<usize>::Continue(()),
    );
}

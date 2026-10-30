//! `#[stream(each)]` and `parse_<rule>_reader_each` (ADR 27 §3): the caller
//! sees every item of the fold as it is taken, and may stop the parse there.
//!
//! What is promised, and checked here under every `Diagnose` mode and read a
//! byte at a time as well as whole:
//!
//! * the items seen are the items the parse in memory folds, in order, each
//!   exactly once - also where a failure makes the parse diagnose again;
//! * reading to the end gives `Continue` with `parse_<rule>_reader`'s value,
//!   or its error;
//! * `Break` ends the parse at that item: what follows is not read, and is
//!   not checked - an error after it is never reached.
//!
//! And `recover(…)` inside a stream's item, which a grammar for a format of
//! many records uses to go on past a broken one.

use std::cell::Cell;
use std::io::Read;
use std::ops::ControlFlow;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Docs {
        // A prefix, a fold whose items borrow the input, a suffix.
        #[stream(each)]
        pub doc -> (String, usize) =
            "[" t:title ";" n:fold(entry, || 0usize, |n: usize, _e: (&str, i64)| n + 1) "]"
            -> { (t, n) }
        title -> String = s:raw_ident -> { s.to_string() }
        entry -> (&'a str, i64) = k:raw_ident "=" v:i64 ","? -> { (k, v) }
    }
}

grammar! {
    grammar Rec {
        #[stream]
        pub doc -> (usize, usize) =
            r:fold(item, || (0usize, 0usize), |(ok, bad): (usize, usize), v: Option<i64>| {
                if v.is_some() { (ok + 1, bad) } else { (ok, bad + 1) }
            })
            eof -> { r }
        item -> Option<i64> = r:recover(entry, ";") -> { r }
        entry -> i64 = v:i64 ";" -> { v }
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

fn entries(n: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| format!("k{i}={}", i as i64 * 7 - 50))
        .collect();
    format!("[ title ; {} ]", items.join(", "))
}

/// What the parse in memory folds, item by item: the same grammar's items
/// parsed one after another.
fn items_in_memory(input: &str) -> Vec<(String, i64)> {
    let body = &input[input.find(';').unwrap() + 1..input.rfind(']').unwrap_or(input.len())];
    body.split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            let (k, v) = s.trim().split_once('=').unwrap();
            (k.to_string(), v.trim().parse().unwrap())
        })
        .collect()
}

type Seen = Vec<(String, i64)>;
type Answer = Result<ControlFlow<usize, (String, usize)>, ParseError>;

fn each_reader(
    input: &str,
    step: usize,
    mode: Diagnose,
    stop_at: Option<usize>,
) -> (Answer, Seen, usize) {
    let read = Cell::new(0);
    let mut seen = Vec::new();
    let r = Docs::parse_doc_reader_each(
        Trickle {
            data: input.as_bytes(),
            step,
            read: &read,
        },
        &mut ctx(mode),
        SMALL,
        |&(k, v): &(&str, i64)| {
            seen.push((k.to_string(), v));
            match stop_at {
                Some(n) if seen.len() == n => ControlFlow::Break(n),
                _ => ControlFlow::Continue(()),
            }
        },
    );
    (r, seen, read.get())
}

fn plain_reader(input: &str, step: usize, mode: Diagnose) -> Result<(String, usize), ParseError> {
    let read = Cell::new(0);
    Docs::parse_doc_reader(
        Trickle {
            data: input.as_bytes(),
            step,
            read: &read,
        },
        &mut ctx(mode),
        SMALL,
    )
}

#[test]
fn every_item_is_seen_once_in_order_and_the_value_is_the_readers() {
    let input = entries(40);
    let want = items_in_memory(&input);
    for mode in MODES {
        for step in STEPS {
            let (r, seen, _) = each_reader(&input, step, mode, None);
            assert_eq!(seen, want, "{mode:?} step {step}");
            let plain = plain_reader(&input, step, mode).unwrap();
            assert_eq!(
                r.unwrap(),
                ControlFlow::Continue(plain),
                "{mode:?} step {step}"
            );
        }
    }
}

#[test]
fn break_stops_at_that_item_and_reads_no_further_than_its_window() {
    let input = entries(100_000);
    for mode in MODES {
        for step in [1, 64] {
            let (r, seen, read) = each_reader(&input, step, mode, Some(3));
            assert_eq!(r.unwrap(), ControlFlow::Break(3), "{mode:?} step {step}");
            assert_eq!(seen.len(), 3);
            assert_eq!(seen, items_in_memory(&input)[..3]);
            // A window of `max_frame` past the item and a block at most.
            assert!(
                read <= 256 + 64 + 64,
                "{mode:?} step {step}: read {read} bytes"
            );
        }
    }
}

#[test]
fn what_follows_a_break_is_never_checked() {
    // Broken after the fifth entry and never closed: a stop before it
    // answers `Break`, a parse to the end the error.
    let mut input = entries(10);
    input.truncate(input.find("k5").unwrap());
    input.push_str("k5 = oops");
    for mode in MODES {
        for step in STEPS {
            let (r, _, _) = each_reader(&input, step, mode, Some(2));
            assert_eq!(r.unwrap(), ControlFlow::Break(2), "{mode:?} step {step}");
        }
    }
}

#[test]
fn an_error_is_the_readers_and_items_before_it_are_seen_once() {
    let mut input = entries(10);
    input.truncate(input.find("k5").unwrap());
    input.push_str("k5 = oops, k6 = 1 ]");
    for mode in MODES {
        for step in STEPS {
            let (r, seen, _) = each_reader(&input, step, mode, None);
            let e = r.unwrap_err();
            let plain = plain_reader(&input, step, mode).unwrap_err();
            assert_eq!(e.offset, plain.offset, "{mode:?} step {step}");
            assert_eq!(e.headline(), plain.headline(), "{mode:?} step {step}");
            // The diagnosing pass parses again; the caller saw each once.
            assert_eq!(seen, items_in_memory(&entries(5)), "{mode:?} step {step}");
        }
    }
}

#[test]
fn a_channel_turns_the_reader_into_an_iterator() {
    // The recipe of SYNTAX.md: the reader on a thread of its own, the items
    // through a bounded channel, a receiver that stops early.
    let input = entries(10_000);
    let (tx, rx) = std::sync::mpsc::sync_channel::<(String, i64)>(16);
    let first: Vec<(String, i64)> = std::thread::scope(|s| {
        let reader = s.spawn(|| {
            let mut context = ParseContext::<()>::default();
            Docs::parse_doc_reader_each(
                input.as_bytes(),
                &mut context,
                Reading::default(),
                |&(k, v)| match tx.send((k.to_string(), v)) {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(_) => ControlFlow::Break(()),
                },
            )
        });
        let first = rx.iter().take(5).collect();
        drop(rx);
        assert_eq!(reader.join().unwrap().unwrap(), ControlFlow::Break(()));
        first
    });
    assert_eq!(first, items_in_memory(&input)[..5]);
}

// --- `recover` inside an item.

fn rec_in_memory(
    input: &str,
    mode: Diagnose,
) -> (Result<(usize, usize), ParseError>, ParseContext<()>) {
    let mut i = ParseInput {
        input: LocatingSlice::new(input),
        state: ctx(mode),
    };
    let r = Rec::parse_doc().parse_next(&mut i);
    (r, i.state)
}

fn rec_reader(
    input: &str,
    step: usize,
    mode: Diagnose,
    reading: Reading,
) -> (Result<(usize, usize), ParseError>, ParseContext<()>) {
    let read = Cell::new(0);
    let mut c = ctx(mode);
    let r = Rec::parse_doc_reader(
        Trickle {
            data: input.as_bytes(),
            step,
            read: &read,
        },
        &mut c,
        reading,
    );
    (r, c)
}

#[test]
fn recover_in_an_item_goes_on_past_a_broken_record_as_in_memory() {
    let inputs = [
        "1; 2; bad; 3;",
        "x; y; 4;",
        "5; 6 7; 8;",
        "9; bad",
        "1; 2; 3;",
        "",
        "; ; 1;",
    ];
    // The cases do recover, and say why where the engine keeps it.
    assert_eq!(
        rec_in_memory("1; 2; bad; 3;", Diagnose::Replay).0.unwrap(),
        (3, 1)
    );
    let (_, eager) = rec_in_memory("x; y; 4;", Diagnose::Eager);
    assert_eq!((eager.recoveries, eager.recovered.len()), (2, 2));
    for input in inputs {
        for mode in MODES {
            let (mem, mc) = rec_in_memory(input, mode);
            for step in STEPS {
                let what = format!("{input:?} {mode:?} step {step}");
                let (got, gc) = rec_reader(input, step, mode, SMALL);
                match (&mem, &got) {
                    (Ok(a), Ok(b)) => assert_eq!(a, b, "{what}"),
                    (Err(a), Err(b)) => {
                        assert_eq!(a.offset, b.offset, "{what}");
                        assert_eq!(a.headline(), b.headline(), "{what}");
                    }
                    _ => panic!("{what}: in memory {mem:?}, read {got:?}"),
                }
                assert_eq!(mc.recoveries, gc.recoveries, "{what}: recoveries");
                let offsets = |c: &ParseContext<()>| -> Vec<(usize, String)> {
                    c.recovered
                        .iter()
                        .map(|e| (e.offset, e.headline()))
                        .collect()
                };
                assert_eq!(offsets(&mc), offsets(&gc), "{what}: recovered");
            }
        }
    }
}

#[test]
fn a_recovery_that_skips_further_than_max_frame_says_so() {
    // The skip to `;` is part of the item, and the item must fit the window.
    let input = format!("1; bad{} ; 2;", "x".repeat(2000));
    let (got, _) = rec_reader(&input, 7, Diagnose::Replay, SMALL);
    let e = got.unwrap_err();
    assert!(e.headline().contains("looked more than"), "{e:?}");
    // With a window large enough, it is the parse in memory's answer.
    let (mem, _) = rec_in_memory(&input, Diagnose::Replay);
    let (got, _) = rec_reader(
        &input,
        7,
        Diagnose::Replay,
        Reading {
            block: 64,
            max_frame: 4096,
        },
    );
    assert_eq!(mem.unwrap(), got.unwrap());
}

#[test]
fn a_user_state_that_is_not_empty_sees_each_item_once_too() {
    // A state of size > 0 makes the reader fill its window first, or, under
    // `Replay`, copy the state per item: other paths through the driver.
    let input = entries(60);
    let want = items_in_memory(&input);
    for mode in MODES {
        for step in STEPS {
            let read = Cell::new(0);
            let mut seen = Vec::new();
            let mut c = ParseContext::<u64> {
                diagnose: mode,
                ..Default::default()
            };
            let r = Docs::parse_doc_reader_each(
                Trickle {
                    data: input.as_bytes(),
                    step,
                    read: &read,
                },
                &mut c,
                SMALL,
                |&(k, v): &(&str, i64)| {
                    seen.push((k.to_string(), v));
                    ControlFlow::<()>::Continue(())
                },
            );
            assert_eq!(r.unwrap(), ControlFlow::Continue(("title".to_string(), 60)));
            assert_eq!(seen, want, "{mode:?} step {step}");
        }
    }
}

//! `fold(..., sep = .., min = .., max = ..)` (#1459 of nikaia): the bounded,
//! separated fold, in memory and read from a `Read` under `#[stream]` and
//! `#[stream(each)]`.
//!
//! What each test checks is the promise of the stream: the reader's answer is
//! the in-memory parse's - the value, or the error at the same offset with the
//! same message - however the input arrives. A fold stopped short of `min`
//! fails with the same error in both.

use std::io::Read;
use std::ops::ControlFlow;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Lists {
        // Any number of items, a comma between two.
        #[stream]
        pub any -> Vec<i64> =
            v:fold(item, Vec::new, |mut v: Vec<i64>, i: i64| { v.push(i); v }, sep = ",") ";"
            -> { v }

        // One to three items.
        #[stream]
        pub bounded -> Vec<i64> =
            v:fold(item, Vec::new, |mut v: Vec<i64>, i: i64| { v.push(i); v },
                   sep = ",", min = 1, max = 3) ";"
            -> { v }

        // At least two, no separator.
        #[stream]
        pub two -> usize =
            n:fold(item, || 0usize, |n: usize, _i: i64| n + 1, min = 2) eof -> { n }

        // The items handed out one by one.
        #[stream(each)]
        pub each -> usize =
            "[" n:fold(item, || 0usize, |n: usize, _i: i64| n + 1, sep = ",", min = 1) "]"
            -> { n }

        // In memory only: the fold in place, with every bound.
        pub in_place -> Vec<i64> =
            v:fold(item, Vec::new, |v: &mut Vec<i64>, i: i64| v.push(i),
                   sep = ";", min = 2, max = 2)
            -> { v }

        item -> i64 = n:i64 -> { n }
    }
}

/// Hands out at most `step` bytes per read, and every third call nothing but
/// `Interrupted`.
struct Trickle<'d> {
    data: &'d [u8],
    step: usize,
    calls: usize,
}

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.calls += 1;
        if self.calls.is_multiple_of(3) {
            return Err(std::io::ErrorKind::Interrupted.into());
        }
        let n = self.step.min(buf.len()).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

fn trickle(input: &str, step: usize) -> Trickle<'_> {
    Trickle {
        data: input.as_bytes(),
        step,
        calls: 0,
    }
}

const MODES: [Diagnose; 4] = [
    Diagnose::Off,
    Diagnose::Replay,
    Diagnose::ReplayInPlace,
    Diagnose::Eager,
];
const STEPS: [usize; 3] = [1, 3, 1 << 20];
const SMALL: Reading = Reading {
    block: 4,
    max_frame: 1 << 16,
};

fn ctx(diagnose: Diagnose) -> ParseContext<()> {
    ParseContext {
        diagnose,
        ..Default::default()
    }
}

fn mem<T>(
    p: &mut impl Parser<ParseInput<'static, ()>, T, ParseError>,
    input: &'static str,
    diagnose: Diagnose,
) -> Result<T, ParseError> {
    p.parse_next(&mut ParseInput {
        input: LocatingSlice::new(input),
        state: ctx(diagnose),
    })
}

/// The reader agrees with the parse in memory.
fn agree<T: PartialEq + std::fmt::Debug>(
    what: &str,
    mode: Diagnose,
    mem: &Result<T, ParseError>,
    got: &Result<T, ParseError>,
) {
    match (mem, got) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "{what}"),
        (Err(a), Err(b)) if mode == Diagnose::Off => {
            assert!(a.is_undiagnosed() && b.is_undiagnosed(), "{what}")
        }
        (Err(a), Err(b)) => {
            assert_eq!(
                a.offset, b.offset,
                "{what}: offsets\n in memory: {a:?}\n read: {b:?}"
            );
            assert_eq!(a.headline(), b.headline(), "{what}: messages");
        }
        _ => panic!("{what}:\n in memory: {mem:?}\n read: {got:?}"),
    }
}

#[test]
fn a_separated_fold_reads_as_it_parses_in_memory() {
    let cases: [(&'static str, Option<Vec<i64>>); 7] = [
        ("1,2,3;", Some(vec![1, 2, 3])),
        ("1 , 2 ,3 ;", Some(vec![1, 2, 3])),
        ("7;", Some(vec![7])),
        (";", Some(vec![])),
        // A trailing separator is left to what follows the fold.
        ("1,2,;", None),
        ("1 2;", None),
        ("1,,2;", None),
    ];
    for (input, want) in cases {
        let m = mem(&mut Lists::parse_any(), input, Diagnose::default());
        assert_eq!(m.as_ref().ok(), want.as_ref(), "{input:?} in memory: {m:?}");
        for mode in MODES {
            let m = mem(&mut Lists::parse_any(), input, mode);
            for step in STEPS {
                let got = Lists::parse_any_reader(trickle(input, step), &mut ctx(mode), SMALL);
                agree(&format!("{input:?}, {mode:?}, step {step}"), mode, &m, &got);
            }
        }
    }
}

#[test]
fn min_and_max_bound_the_fold_in_memory_and_read() {
    let cases: [(&'static str, Option<Vec<i64>>); 6] = [
        ("1;", Some(vec![1])),
        ("1,2,3;", Some(vec![1, 2, 3])),
        // Too few: the error is where the first item should be.
        (";", None),
        ("", None),
        // Too many: the fold stops after three, and `;` is missing there.
        ("1,2,3,4;", None),
        ("1,2,3,", None),
    ];
    for (input, want) in cases {
        let m = mem(&mut Lists::parse_bounded(), input, Diagnose::default());
        assert_eq!(m.as_ref().ok(), want.as_ref(), "{input:?} in memory: {m:?}");
        for mode in MODES {
            let m = mem(&mut Lists::parse_bounded(), input, mode);
            for step in STEPS {
                let got = Lists::parse_bounded_reader(trickle(input, step), &mut ctx(mode), SMALL);
                agree(&format!("{input:?}, {mode:?}, step {step}"), mode, &m, &got);
            }
        }
    }
    let e = mem(&mut Lists::parse_bounded(), "", Diagnose::default()).unwrap_err();
    assert_eq!(e.offset, 0, "{e:?}");
    let e = mem(&mut Lists::parse_bounded(), "1,2,3,4;", Diagnose::default()).unwrap_err();
    assert_eq!(e.offset, 5, "{e:?}");
}

#[test]
fn a_stream_stopped_short_of_min_fails_as_in_memory() {
    for (input, want) in [
        ("1 2", Some(2)),
        ("1 2 3 4", Some(4)),
        ("1", None),
        ("", None),
        ("1 x", None),
    ] {
        let m = mem(&mut Lists::parse_two(), input, Diagnose::default());
        assert_eq!(m.as_ref().ok(), want.as_ref(), "{input:?} in memory: {m:?}");
        for mode in MODES {
            let m = mem(&mut Lists::parse_two(), input, mode);
            for step in STEPS {
                let got = Lists::parse_two_reader(trickle(input, step), &mut ctx(mode), SMALL);
                agree(&format!("{input:?}, {mode:?}, step {step}"), mode, &m, &got);
            }
        }
    }
}

#[test]
fn each_item_of_a_separated_fold_is_handed_out_once() {
    for (input, want) in [
        ("[1]", Some(vec![1])),
        ("[1, 2,3]", Some(vec![1, 2, 3])),
        ("[]", None),
        ("[1,2,]", None),
    ] {
        let m = mem(&mut Lists::parse_each(), input, Diagnose::default());
        assert_eq!(
            m.as_ref().ok(),
            want.as_ref().map(Vec::len).as_ref(),
            "{input:?} in memory: {m:?}"
        );
        for mode in MODES {
            let m = mem(&mut Lists::parse_each(), input, mode);
            for step in STEPS {
                let mut seen = Vec::new();
                let got = Lists::parse_each_reader_each(
                    trickle(input, step),
                    &mut ctx(mode),
                    SMALL,
                    |&i: &i64| {
                        seen.push(i);
                        ControlFlow::<()>::Continue(())
                    },
                );
                let what = format!("{input:?}, {mode:?}, step {step}");
                let got = got.map(|r| match r {
                    ControlFlow::Continue(n) => n,
                    ControlFlow::Break(()) => unreachable!(),
                });
                agree(&what, mode, &m, &got);
                if let Some(w) = &want {
                    assert_eq!(&seen, w, "{what}");
                }
            }
        }
    }
}

#[test]
fn the_fold_in_place_takes_the_same_bounds() {
    assert_eq!(
        mem(&mut Lists::parse_in_place(), "4;5", Diagnose::default()).unwrap(),
        vec![4, 5]
    );
    // `max = 2` leaves the separator and the third item to what follows:
    // the end the entry point demands, so the parse fails at the `;`.
    let e = mem(&mut Lists::parse_in_place(), "4;5;6", Diagnose::default()).unwrap_err();
    assert_eq!(e.offset, 3, "{e:?}");
    // `min = 2`: one item is too few.
    assert!(mem(&mut Lists::parse_in_place(), "4", Diagnose::default()).is_err());
    assert!(mem(&mut Lists::parse_in_place(), "", Diagnose::default()).is_err());
}

//! `fold(..., min = expr, max = expr)` (ADR 26 D3, #1499 of nikaia): the
//! bounds of a fold computed from a value bound before it, in memory, read
//! from a `Read` under `#[stream]`, and from a rule parameter.

use std::io::Read;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Counted {
        // A count, then exactly that many items.
        #[stream]
        pub exact -> Vec<i64> =
            n:usize ":" v:fold(item, Vec::new, |mut v: Vec<i64>, i: i64| { v.push(i); v },
                               sep = ",", min = n, max = n) ";"
            -> { v }

        // At least the count, at most four.
        #[stream]
        pub at_least -> usize =
            n:usize ":" c:fold(item, || 0usize, |c: usize, _i: i64| c + 1, min = n, max = 4) ";"
            -> { c }

        // Between two counts: a `min` above the `max` fails.
        pub between -> usize =
            lo:usize hi:usize ":" c:fold(item, || 0usize, |c: usize, _i: i64| c + 1, min = lo, max = hi) ";"
            -> { c }

        // In place, its bounds from a parameter.
        pub outer -> Vec<i64> = n:usize ":" v:items(n) -> { v }
        rule items(n: usize) -> Vec<i64> =
            v:fold(item, Vec::new, |v: &mut Vec<i64>, i: i64| v.push(i), min = n, max = n) ";"
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
fn a_counted_fold_takes_exactly_its_count() {
    let cases: [(&'static str, Option<Vec<i64>>); 7] = [
        ("3:1,2,3;", Some(vec![1, 2, 3])),
        ("1:7;", Some(vec![7])),
        ("0:;", Some(vec![])),
        // Too few: the error is where the next item should be.
        ("3:1,2;", None),
        ("2:;", None),
        // Too many: the fold stops at the count, and `;` is missing there.
        ("2:1,2,3;", None),
        ("0:1;", None),
    ];
    for (input, want) in cases {
        let m = mem(&mut Counted::parse_exact(), input, Diagnose::default());
        assert_eq!(m.as_ref().ok(), want.as_ref(), "{input:?} in memory: {m:?}");
        for mode in MODES {
            let m = mem(&mut Counted::parse_exact(), input, mode);
            for step in STEPS {
                let got = Counted::parse_exact_reader(trickle(input, step), &mut ctx(mode), SMALL);
                agree(&format!("{input:?}, {mode:?}, step {step}"), mode, &m, &got);
            }
        }
    }
    let e = mem(&mut Counted::parse_exact(), "2:1,2,3;", Diagnose::default()).unwrap_err();
    assert_eq!(e.offset, 5, "{e:?}");
}

#[test]
fn a_computed_min_meets_a_literal_max() {
    let cases: [(&'static str, Option<usize>); 6] = [
        ("2:1 2;", Some(2)),
        ("2:1 2 3 4;", Some(4)),
        ("0:;", Some(0)),
        ("2:1;", None),
        ("1:1 2 3 4 5;", None),
        // The count above the literal `max`: no number of items will do.
        ("5:1 2 3 4 5;", None),
    ];
    for (input, want) in cases {
        let m = mem(&mut Counted::parse_at_least(), input, Diagnose::default());
        assert_eq!(m.as_ref().ok(), want.as_ref(), "{input:?} in memory: {m:?}");
        for mode in MODES {
            let m = mem(&mut Counted::parse_at_least(), input, mode);
            for step in STEPS {
                let got =
                    Counted::parse_at_least_reader(trickle(input, step), &mut ctx(mode), SMALL);
                agree(&format!("{input:?}, {mode:?}, step {step}"), mode, &m, &got);
            }
        }
    }
}

#[test]
fn a_min_above_the_max_fails() {
    let d = Diagnose::default();
    assert_eq!(
        mem(&mut Counted::parse_between(), "1 3:1 2;", d).unwrap(),
        2
    );
    assert_eq!(
        mem(&mut Counted::parse_between(), "2 2:1 2;", d).unwrap(),
        2
    );
    assert!(mem(&mut Counted::parse_between(), "3 1:1 2;", d).is_err());
    assert!(mem(&mut Counted::parse_between(), "3 1:;", d).is_err());
}

#[test]
fn a_parameter_bounds_the_fold_in_place() {
    let d = Diagnose::default();
    assert_eq!(
        mem(&mut Counted::parse_outer(), "2:4 5;", d).unwrap(),
        vec![4, 5]
    );
    assert_eq!(mem(&mut Counted::parse_outer(), "0:;", d).unwrap(), vec![]);
    assert!(mem(&mut Counted::parse_outer(), "2:4;", d).is_err());
    assert!(mem(&mut Counted::parse_outer(), "2:4 5 6;", d).is_err());
}

//! A fold whose step is written `|acc: &mut Acc, item| …` changes the
//! accumulator in place (ADR 24 §11). The claim is that nothing observable
//! changes against the step that takes the accumulator and hands it back:
//! every rule below exists twice, once with each step, and both are run on
//! every string of up to six characters, in the fast pass with its replay and
//! with `Diagnose::Eager`, and must agree on the value, on how much was
//! consumed, and on the rendered error - its item number included.

use winnow::stream::{LocatingSlice, Stream};
use winnow::Parser;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Folds {
        // A record is a name, `;` and one digit, ended by a newline.
        #[frame(boundary = "\n")]
        pub ROW -> (&'a str, u32) =
            name:until(";" | frame_end) ";" d:digit frame_end -> { (name, d as u32 - '0' as u32) }

        // `par_fold`: a count and a sum, as a table would be kept.
        pub PAR -> (usize, u32) = s:par_fold(
            ROW,
            || (0usize, 0u32),
            |acc: (usize, u32), r: (&'a str, u32)| (acc.0 + 1, acc.1 + r.1),
            |a: (usize, u32), b: (usize, u32)| (a.0 + b.0, a.1 + b.1)
        ) -> { s }
        pub PAR_IN_PLACE -> (usize, u32) = s:par_fold(
            ROW,
            || (0usize, 0u32),
            |acc: &mut (usize, u32), r: (&'a str, u32)| { acc.0 += 1; acc.1 += r.1; },
            |a: (usize, u32), b: (usize, u32)| (a.0 + b.0, a.1 + b.1)
        ) -> { s }

        // A plain `fold`, in a syntactic rule, keeping the names.
        pub rule names -> Vec<&'a str> = v:fold(
            raw_ident,
            Vec::new,
            |mut acc: Vec<&'a str>, n: &'a str| { acc.push(n); acc }
        ) -> { v }
        pub rule names_in_place -> Vec<&'a str> = v:fold(
            raw_ident,
            Vec::new,
            |acc: &mut Vec<&'a str>, n: &'a str| acc.push(n)
        ) -> { v }

        // An inferred accumulator: `&mut _` is enough for the macro.
        pub rule count_in_place -> usize = n:fold(
            raw_ident,
            || 0usize,
            |acc: &mut _, _n: &'a str| *acc += 1
        ) -> { n }
        pub rule count -> usize = n:fold(raw_ident, || 0usize, |acc: usize, _n: &'a str| acc + 1) -> { n }
    }
}

fn run<'a, O, P>(mut p: P, input: &'a str, diagnose: Diagnose) -> (Result<O, String>, usize)
where
    P: Parser<ParseInput<'a, ()>, O, ParseError>,
{
    let mut stream = ParseInput {
        input: LocatingSlice::new(input),
        state: ParseContext::<()> {
            diagnose,
            ..Default::default()
        },
    };
    // The rule's own name is the one thing the two spellings may not share.
    let result = p.parse_next(&mut stream).map_err(|e| {
        e.render(input)
            .replace("_IN_PLACE", "")
            .replace("_in_place", "")
    });
    (result, input.len() - stream.eof_offset())
}

fn strings(alphabet: &[char], len: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut layer = vec![String::new()];
    for _ in 0..len {
        let mut next = Vec::with_capacity(layer.len() * alphabet.len());
        for s in &layer {
            for c in alphabet {
                let mut t = s.clone();
                t.push(*c);
                next.push(t);
            }
        }
        out.extend(next.iter().cloned());
        layer = next;
    }
    out
}

const ALPHABET: [char; 6] = ['a', ';', '7', '\n', ' ', 'é'];

macro_rules! agree {
    ($by_value:ident, $in_place:ident) => {
        for s in strings(&ALPHABET, 6) {
            for diagnose in [Diagnose::Replay, Diagnose::Eager] {
                let by_value = run(Folds::$by_value(), &s, diagnose);
                let in_place = run(Folds::$in_place(), &s, diagnose);
                assert_eq!(
                    by_value,
                    in_place,
                    "{} and {} disagree on {s:?} ({diagnose:?})",
                    stringify!($by_value),
                    stringify!($in_place)
                );
            }
            // And the fast pass agrees with the diagnosing one.
            assert_eq!(
                run(Folds::$in_place(), &s, Diagnose::Replay),
                run(Folds::$in_place(), &s, Diagnose::Eager),
                "{} disagrees with itself on {s:?}",
                stringify!($in_place)
            );
        }
    };
}

#[test]
fn a_par_fold_in_place_agrees() {
    agree!(parse_PAR, parse_PAR_IN_PLACE);
}

#[test]
fn a_fold_in_place_agrees() {
    agree!(parse_names, parse_names_in_place);
}

#[test]
fn an_inferred_accumulator_agrees() {
    agree!(parse_count, parse_count_in_place);
}

#[test]
fn the_accumulator_is_the_steps() {
    assert_eq!(
        run(
            Folds::parse_PAR_IN_PLACE(),
            "a;1\nb;2\nc;3\n",
            Diagnose::Replay
        )
        .0,
        Ok((3, 6))
    );
    assert_eq!(
        run(Folds::parse_names_in_place(), "x y z", Diagnose::Replay).0,
        Ok(vec!["x", "y", "z"])
    );
    // A failing item is numbered as in the by-value fold.
    let err = run(
        Folds::parse_PAR_IN_PLACE(),
        "a;1\nb;2\nc;x\n",
        Diagnose::Replay,
    )
    .0;
    assert!(err.as_ref().is_err_and(|e| e.contains("item 3")), "{err:?}");
}

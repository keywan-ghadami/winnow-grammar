//! A one-byte literal is compared as a byte in the fast pass and matched by
//! winnow's `literal` in the diagnosing one (ADR 24 §8a). The claim is that
//! nothing observable changes, and `Diagnose::Eager` - no fast pass at all -
//! is the reference: every rule below is run both ways on every string of up
//! to six characters, and must give the same value, consume the same, and
//! fail with the same message.

use winnow::stream::{LocatingSlice, Stream};
use winnow::Parser;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Sep {
        // Lexical: a separator between two runs, as a string and a char.
        pub PAIR -> (&'a str, &'a str, &'a str) =
            a:raw_ident s:";" b:raw_ident ',' -> { (a, s, b) }

        // Optional and repeated separators.
        pub LIST -> (usize, Option<&'a str>) =
            raw_ident xs:(";" raw_ident)* t:"."? -> { (xs.len(), t) }

        // Syntactic: the whitespace skip runs between the tokens.
        pub rule spaced -> (&'a str, &'a str) = a:raw_ident ";" b:raw_ident -> { (a, b) }

        // `frame_end` with a one-byte boundary, under a fold.
        #[frame(boundary = "\n")]
        pub ROW -> &'a str = n:until(";" | frame_end) ";" raw_ident frame_end -> { n }
        pub FILE -> usize = n:par_fold(ROW, || 0usize, |a: usize, _r: &'a str| a + 1, |a: usize, b: usize| a + b) -> { n }
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
    let result = p.parse_next(&mut stream).map_err(|e| e.render(input));
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

const ALPHABET: [char; 7] = ['a', ';', ',', '.', '\n', ' ', 'é'];

macro_rules! agree {
    ($rule:ident) => {
        for s in strings(&ALPHABET, 6) {
            let fast = run(Sep::$rule(), &s, Diagnose::Replay);
            let eager = run(Sep::$rule(), &s, Diagnose::Eager);
            assert_eq!(
                fast,
                eager,
                "{} disagrees with itself on {s:?}",
                stringify!($rule)
            );
        }
    };
}

#[test]
fn a_separator_between_runs_agrees() {
    agree!(parse_PAIR);
}

#[test]
fn optional_and_repeated_separators_agree() {
    agree!(parse_LIST);
}

#[test]
fn a_separator_between_skipped_whitespace_agrees() {
    agree!(parse_spaced);
}

#[test]
fn frame_end_under_a_fold_agrees() {
    agree!(parse_FILE);
}

#[test]
fn the_value_is_the_byte() {
    assert_eq!(
        run(Sep::parse_PAIR(), "a;b,", Diagnose::Replay).0,
        Ok(("a", ";", "b"))
    );
    assert_eq!(
        run(Sep::parse_FILE(), "a;b\nc;d\n", Diagnose::Replay).0,
        Ok(2)
    );
}

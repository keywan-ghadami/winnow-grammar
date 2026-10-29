//! `parse_<rule>_reader`: a `par_fold` rule's input read from a `Read` in
//! blocks (ADR 27 §1).
//!
//! What the tests establish: the result is the parse of the whole input in
//! memory, for every block size and every size of read the reader hands
//! back, `Parallelism` on or off - the empty input, a missing last boundary
//! and rejected inputs included; a boundary longer than one byte is found
//! when a read splits it; errors are at their offset in the stream; the
//! buffer is bounded by `max_frame`; a reader's error is reported; a text
//! grammar's bytes are checked as UTF-8 at their offset in the stream.

use std::collections::BTreeMap;
use std::io::Read;
use winnow_grammar::grammar;
use winnow_grammar::rt::{Parallelism, Reading};
use winnow_grammar::{ParseContext, ParseError};

/// Hands out at most `step` bytes per read, and every third call nothing but
/// `Interrupted` - a pipe, as unhelpful as one can be.
struct Trickle<'d> {
    data: &'d [u8],
    step: usize,
    calls: usize,
}

impl<'d> Trickle<'d> {
    fn new(data: &'d [u8], step: usize) -> Self {
        Trickle {
            data,
            step,
            calls: 0,
        }
    }
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

const HOWS: [Parallelism; 2] = [Parallelism::Off, Parallelism::Pieces(3)];

/// Block sizes and read sizes worth trying: a byte at a time, sizes that
/// split every frame somewhere, and more than the input.
const SIZES: [usize; 6] = [1, 2, 3, 5, 8, 1 << 16];

fn reading(block: usize) -> Reading {
    Reading {
        block,
        ..Reading::default()
    }
}

// --- A text grammar ---------------------------------------------------------

grammar! {
    grammar Totals {
        NAME -> &'a str = s:until(";" | frame_end) -> { s }

        #[frame(boundary = "\n")]
        pub ROW -> (&'a str, i64) = n:NAME ";" v:dec<i64>(digit1) frame_end -> { (n, v) }

        // The names are copied: what a reader's fold returns outlives the
        // block it was read from.
        pub FILE -> BTreeMap<String, i64> = m:par_fold(
            ROW,
            BTreeMap::new,
            |mut acc: BTreeMap<String, i64>, (n, v): (&str, i64)| {
                *acc.entry(n.to_string()).or_insert(0) += v;
                acc
            },
            |mut a: BTreeMap<String, i64>, b: BTreeMap<String, i64>| {
                for (k, v) in b {
                    *a.entry(k).or_insert(0) += v;
                }
                a
            }
        ) -> { m }
    }
}

fn totals_whole(input: &str) -> Result<BTreeMap<String, i64>, ParseError> {
    Totals::parse_FILE_pieces(input, &ParseContext::<()>::default(), Parallelism::Off)
}

fn totals_read(
    input: &str,
    step: usize,
    block: usize,
    how: Parallelism,
) -> Result<BTreeMap<String, i64>, ParseError> {
    Totals::parse_FILE_reader(
        Trickle::new(input.as_bytes(), step),
        &ParseContext::<()>::default(),
        how,
        reading(block),
    )
}

fn sample(rows: usize) -> String {
    let names = ["Hamburg", "Zürich", "東京", "A", "São Paulo"];
    (0..rows)
        .map(|i| format!("{};{}\n", names[(i * 3) % names.len()], i * 7 % 100))
        .collect()
}

#[test]
fn reading_agrees_with_the_parse_in_memory() {
    for input in [
        sample(40),
        sample(1),
        String::new(),
        // No boundary at the end: the last frame is the rest.
        "A;1\nB;2".to_string(),
        // Rejected: a blank line, a frame without its number.
        "A;1\n\nB;2\n".to_string(),
        "A;1\nB;\nC;3\n".to_string(),
    ] {
        let expected = totals_whole(&input);
        for step in SIZES {
            for block in SIZES {
                for how in HOWS {
                    let got = totals_read(&input, step, block, how);
                    let at = format!("{input:?}, step {step}, block {block}, {how:?}");
                    match (&expected, &got) {
                        (Ok(e), Ok(g)) => assert_eq!(e, g, "{at}"),
                        (Err(e), Err(g)) => assert_eq!(e.offset, g.offset, "{at}"),
                        _ => panic!("{at}: in memory {expected:?}, read {got:?}"),
                    }
                }
            }
        }
    }
}

#[test]
fn an_error_is_at_its_offset_in_the_stream() {
    let mut input = sample(50);
    let line = input.len() - 30;
    let line = line + input[line..].find('\n').unwrap() + 1;
    input.insert_str(line, "broken\n");
    let expected = totals_whole(&input).unwrap_err();
    assert!(expected.offset > line, "{expected:?}");
    for block in [1, 7, 64] {
        let got = totals_read(&input, 5, block, Parallelism::Off).unwrap_err();
        assert_eq!(got.offset, expected.offset, "block {block}");
        assert_eq!(got.headline(), expected.headline(), "block {block}");
        assert_eq!(
            got.render_excerpt(),
            expected.render(&input),
            "block {block}"
        );
    }
}

#[test]
fn invalid_utf8_is_reported_at_its_offset_in_the_stream() {
    let mut input = sample(30).into_bytes();
    let at = input.len() - 10;
    input[at] = 0xff;
    for block in [1, 4, 1 << 16] {
        let e = Totals::parse_FILE_reader(
            Trickle::new(&input, 3),
            &ParseContext::<()>::default(),
            Parallelism::Off,
            reading(block),
        )
        .unwrap_err();
        assert_eq!(e.offset, at, "block {block}");
        assert_eq!(
            e.headline(),
            format!("invalid UTF-8 at byte {at}: 0xff"),
            "block {block}"
        );
    }
}

#[test]
fn more_than_max_frame_without_a_boundary_is_an_error_not_a_bigger_buffer() {
    let input = format!("A;1\nB;2\n{}", "x".repeat(100));
    let e = Totals::parse_FILE_reader(
        Trickle::new(input.as_bytes(), 8),
        &ParseContext::<()>::default(),
        Parallelism::Off,
        Reading {
            block: 4,
            max_frame: 16,
        },
    )
    .unwrap_err();
    assert_eq!(e.offset, 8, "{e:?}");
    assert!(
        e.headline().contains("more than 16 bytes"),
        "{}",
        e.headline()
    );

    // A frame up to the limit is waited out.
    let input = format!("A;1\n{};2\n", "y".repeat(10));
    let got = Totals::parse_FILE_reader(
        Trickle::new(input.as_bytes(), 1),
        &ParseContext::<()>::default(),
        Parallelism::Off,
        Reading {
            block: 1,
            max_frame: 16,
        },
    );
    assert_eq!(got, totals_whole(&input));
}

struct Failing;

impl Read for Failing {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("the disk is on fire"))
    }
}

#[test]
fn a_read_error_is_reported() {
    let e = Totals::parse_FILE_reader(
        Failing,
        &ParseContext::<()>::default(),
        Parallelism::Off,
        Reading::default(),
    )
    .unwrap_err();
    assert!(
        e.headline().contains("the disk is on fire"),
        "{}",
        e.headline()
    );
}

// --- A boundary of more than one byte ---------------------------------------

grammar! {
    grammar Records {
        #[frame(boundary = "%%")]
        pub REC -> u64 = n:dec<u64>(digit1) frame_end -> { n }
        pub FILE -> u64 = s:par_fold(REC, || 0u64, |a: u64, v: u64| a + v, |a: u64, b: u64| a + b) -> { s }
    }
}

#[test]
fn a_boundary_split_by_a_read_is_found() {
    let input: String = (0..50).map(|i| format!("{}%%", i * 13)).collect();
    let expected: u64 = (0..50).map(|i| i * 13).sum();
    for step in SIZES {
        for block in SIZES {
            let got = Records::parse_FILE_reader(
                Trickle::new(input.as_bytes(), step),
                &ParseContext::<()>::default(),
                Parallelism::Off,
                reading(block),
            );
            assert_eq!(got.unwrap(), expected, "step {step}, block {block}");
        }
    }
    // The bound counts from the last boundary, which a split one is not yet:
    // ten bytes of digits and half a boundary wait under a limit of eleven.
    let got = Records::parse_FILE_reader(
        Trickle::new(b"1234567890%%5%%", 1),
        &ParseContext::<()>::default(),
        Parallelism::Off,
        Reading {
            block: 1,
            max_frame: 11,
        },
    );
    assert_eq!(got.unwrap(), 1234567895);
}

// --- A byte grammar ---------------------------------------------------------

grammar! {
    grammar Sums {
        input bytes;
        #[frame(boundary = "\n")]
        pub ROW -> i64 = v:dec<i64>(digit1) frame_end -> { v }
        pub FILE -> i64 = s:par_fold(ROW, || 0i64, |a: i64, v: i64| a + v, |a: i64, b: i64| a + b) -> { s }
    }
}

#[test]
fn a_byte_grammar_reads_bytes() {
    let input: Vec<u8> = (0..100)
        .flat_map(|i| format!("{i}\n").into_bytes())
        .collect();
    let expected =
        Sums::parse_FILE_pieces(&input, &ParseContext::<()>::default(), Parallelism::Off);
    for block in SIZES {
        for how in HOWS {
            let got = Sums::parse_FILE_reader(
                Trickle::new(&input, 4),
                &ParseContext::<()>::default(),
                how,
                reading(block),
            );
            assert_eq!(got, expected, "block {block}, {how:?}");
        }
    }
    assert_eq!(expected.unwrap(), (0..100).sum::<i64>());
}

#[test]
fn items_are_numbered_across_pieces_as_in_one_parse() {
    let mut input = sample(60);
    let line = input.len() - 30;
    let line = line + input[line..].find('\n').unwrap() + 1;
    input.insert_str(line, "broken\n");
    let expected = totals_whole(&input).unwrap_err();
    assert!(expected.to_string().contains("in item 5"), "{expected}");
    for how in [
        Parallelism::Pieces(2),
        Parallelism::Pieces(7),
        Parallelism::Claimed {
            piece: 40,
            order: winnow_grammar::rt::Order::Kept,
        },
        Parallelism::Claimed {
            piece: 40,
            order: winnow_grammar::rt::Order::Any,
        },
    ] {
        let got =
            Totals::parse_FILE_pieces(&input, &ParseContext::<()>::default(), how).unwrap_err();
        assert_eq!(got.to_string(), expected.to_string(), "{how:?}");
    }
}

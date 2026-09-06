//! `#[frame]` and `par_fold`: cutting an input into pieces that are parsed on
//! their own and merged.
//!
//! What the tests establish: the split's edge cases (`rt::frames`), that
//! split + parse + merge gives the same answer as one sequential parse, and
//! that `until(…)` inside a frame is *bounded* by the frame's boundary rather
//! than running through it.

use std::collections::BTreeMap;
use winnow_grammar::grammar;
use winnow_grammar::rt::frames;
use winnow_grammar::testing::WinnowTestExt;

// -----------------------------------------------------------------------------
// The split
// -----------------------------------------------------------------------------

fn pieces(input: &str, n: usize) -> Vec<&str> {
    frames(input, "\n", n)
        .into_iter()
        .map(|r| &input[r])
        .collect()
}

#[test]
fn one_piece_is_the_whole_input() {
    assert_eq!(pieces("a\nb\nc\n", 1), vec!["a\nb\nc\n"]);
    // `n == 0` is read as one.
    assert_eq!(pieces("a\nb\nc\n", 0), vec!["a\nb\nc\n"]);
}

#[test]
fn a_piece_starts_just_past_the_first_boundary_after_its_nominal_start() {
    // 6 bytes, nominal start of the second piece at 3 ("b"); the first
    // boundary from there is after "b".
    assert_eq!(pieces("a\nb\nc\n", 2), vec!["a\nb\n", "c\n"]);
}

#[test]
fn a_frame_ending_exactly_at_the_nominal_start_belongs_to_the_piece_before() {
    // The nominal start lands on the "\n" after "abc": that frame's end is
    // found by the first piece, so the first piece owns it.
    assert_eq!(pieces("abc\nde\n", 2), vec!["abc\n", "de\n"]);
}

#[test]
fn input_without_a_trailing_boundary_keeps_its_last_frame() {
    assert_eq!(pieces("a\nb", 2), vec!["a\n", "b"]);
}

#[test]
fn a_frame_longer_than_a_piece_leaves_the_following_pieces_empty() {
    // Every nominal start lands inside the one frame; every repaired start is
    // the end of the input.
    assert_eq!(pieces("aaaaaaaaaa\n", 4), vec!["aaaaaaaaaa\n", "", "", ""]);
}

#[test]
fn empty_input_is_one_empty_piece() {
    assert_eq!(pieces("", 3), vec!["", "", ""]);
}

#[test]
fn every_range_is_a_character_boundary() {
    // With three pieces the second nominal start (byte 4) is inside `ü`. The
    // scan runs on bytes and only ever cuts after the boundary, so slicing the
    // `&str` by the ranges must not panic.
    assert_eq!(pieces("é\nü\n", 3), vec!["é\n", "ü\n", ""]);
}

#[test]
fn the_pieces_cover_the_input_exactly_once() {
    let input: String = (0..1000).map(|i| format!("line{i}\n")).collect();
    for n in [1, 2, 3, 7, 64, 5000] {
        let ranges = frames(&input, "\n", n);
        assert_eq!(ranges.len(), n.max(1));
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[ranges.len() - 1].end, input.len());
        for w in ranges.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
        // Each non-empty piece starts at a line start and ends at a line end.
        for r in &ranges {
            if r.is_empty() {
                continue;
            }
            assert!(r.start == 0 || input.as_bytes()[r.start - 1] == b'\n');
            assert!(input.as_bytes()[r.end - 1] == b'\n');
        }
    }
}

// -----------------------------------------------------------------------------
// par_fold: split + parse + merge == sequential
// -----------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, Clone)]
pub struct Summary {
    stations: BTreeMap<String, (i32, i32, i64, i64)>, // min, max, sum, count
}

impl Summary {
    fn record(&mut self, name: &str, tenths: i32) {
        let e = self
            .stations
            .entry(name.to_string())
            .or_insert((tenths, tenths, 0, 0));
        e.0 = e.0.min(tenths);
        e.1 = e.1.max(tenths);
        e.2 += tenths as i64;
        e.3 += 1;
    }

    fn merge(mut self, other: Summary) -> Summary {
        for (name, (min, max, sum, count)) in other.stations {
            let e = self.stations.entry(name).or_insert((min, max, 0, 0));
            e.0 = e.0.min(min);
            e.1 = e.1.max(max);
            e.2 += sum;
            e.3 += count;
        }
        self
    }
}

grammar! {
    grammar Measurements {
        NAME -> &'a str = s:until(";") -> { s }

        TENTHS -> i32 =
            neg:"-"? whole:digit{1,2} "." frac:digit
            -> {
                let mut v: i32 = 0;
                for d in whole { v = v * 10 + (d as i32 - '0' as i32); }
                v = v * 10 + (frac as i32 - '0' as i32);
                if neg.is_some() { -v } else { v }
            }

        // A measurement can be found from any offset by scanning to the next
        // "\n": the boundary is inferred from the trailing literal.
        #[frame]
        pub MEASUREMENT -> (&'a str, i32) =
            name:NAME ";" temp:TENTHS "\n" -> { (name, temp) }

        pub FILE -> Summary =
            s:par_fold(
                MEASUREMENT,
                Summary::default,
                |mut acc: Summary, (name, temp): (&str, i32)| { acc.record(name, temp); acc },
                |a: Summary, b: Summary| a.merge(b)
            ) -> { s }
    }
}

fn sample_input(lines: usize) -> String {
    let names = [
        "Hamburg",
        "Zürich",
        "São Paulo",
        "東京",
        "Abidjan",
        "Reykjavík",
        "A",
        "Ust-Kamenogorsk",
    ];
    let mut s = String::new();
    for i in 0..lines {
        let name = names[(i * 7) % names.len()];
        let tenths = ((i as i64 * 37) % 1999) as i32 - 999; // -99.9 ..= 99.9
        let sign = if tenths < 0 { "-" } else { "" };
        s.push_str(&format!(
            "{name};{sign}{}.{}\n",
            tenths.abs() / 10,
            tenths.abs() % 10
        ));
    }
    s
}

fn parse_sequential(input: &str) -> Summary {
    Measurements::parse_FILE()
        .parse_test(input)
        .assert_success()
        .clone()
}

fn parse_in_pieces(input: &str, n: usize) -> Summary {
    let ranges = Measurements::frames_FILE(input, n);
    assert_eq!(ranges.len(), n.max(1));
    ranges
        .into_iter()
        .map(|r| {
            Measurements::parse_FILE()
                .parse_test(&input[r])
                .assert_success()
                .clone()
        })
        .fold(Summary::default(), Measurements::merge_FILE)
}

#[test]
fn pieces_merge_to_the_sequential_result() {
    let input = sample_input(20_000);
    let expected = parse_sequential(&input);
    assert_eq!(expected.stations.len(), 8);

    for n in [1, 2, 3, 7, 16, 1000] {
        assert_eq!(parse_in_pieces(&input, n), expected, "n = {n}");
    }
}

#[test]
fn more_pieces_than_frames_still_merges_correctly() {
    // Most pieces come out empty; an empty piece folds to the initial
    // accumulator, which the merge must treat as the identity.
    let input = sample_input(10);
    let expected = parse_sequential(&input);
    assert_eq!(parse_in_pieces(&input, 64), expected);
}

#[test]
fn an_empty_piece_parses_to_the_initial_accumulator() {
    assert_eq!(parse_sequential(""), Summary::default());
}

// -----------------------------------------------------------------------------
// until(…) inside a frame is bounded by the boundary
// -----------------------------------------------------------------------------

grammar! {
    grammar Unframed {
        // The same NAME rule with no frame around it: `until(";")` consumes
        // anything, a newline included.
        NAME -> &'a str = s:until(";") -> { s }
        pub REC -> String = name:NAME ";" -> { name.to_string() }
    }
}

#[test]
fn outside_a_frame_until_runs_through_a_newline() {
    Unframed::parse_REC()
        .parse_test("Ham\nburg;")
        .assert_success_is("Ham\nburg".to_string());
}

#[test]
fn inside_a_frame_until_stops_at_the_boundary() {
    // Inside `MEASUREMENT`'s frame the skip in NAME stops at "\n" as well as
    // at ";": the newline is where the parse fails, instead of silently
    // joining two frames into one name.
    Measurements::parse_MEASUREMENT()
        .parse_test("Ham\nburg;1.0\n")
        .assert_failure_contains("column 4");
}

#[test]
fn a_bounded_until_still_yields_the_text_before_its_terminator() {
    Measurements::parse_MEASUREMENT()
        .parse_test("Hamburg;12.3\n")
        .assert_success_with(|(name, temp), _| {
            assert_eq!(*name, "Hamburg");
            assert_eq!(*temp, 123);
        });
}

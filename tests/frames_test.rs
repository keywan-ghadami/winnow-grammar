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
        // Up to `;`, or to the end of the frame: under a frame the
        // terminator has to cover the boundary, and the grammar says so.
        // `frame_end` is the boundary of the frame this rule is reached
        // from - written once, in the attribute, and referenced here.
        NAME -> &'a str = s:until(";" | frame_end) -> { s }

        TENTHS -> i32 =
            neg:"-"? whole:digit{1,2} "." frac:digit
            -> {
                let mut v: i32 = 0;
                for d in whole { v = v * 10 + (d as i32 - '0' as i32); }
                v = v * 10 + (frac as i32 - '0' as i32);
                if neg.is_some() { -v } else { v }
            }

        // A measurement can be found from any offset by scanning to the next
        // "\n". The boundary is written once; the rule ends in `frame_end`.
        #[frame(boundary = "\n")]
        pub MEASUREMENT -> (&'a str, i32) =
            name:NAME ";" temp:TENTHS frame_end -> { (name, temp) }

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
// The terminator says where the skip stops - the same everywhere
// -----------------------------------------------------------------------------

grammar! {
    grammar Unframed {
        // The same NAME rule with no frame anywhere near it. It means the
        // same thing: a frame elsewhere changes no rule's parser.
        NAME -> &'a str = s:until(";" | "\n") -> { s }
        pub REC -> String = name:NAME ";" -> { name.to_string() }
    }
}

#[test]
fn the_terminator_stops_at_either_alternative_outside_a_frame_too() {
    Unframed::parse_REC()
        .parse_test("Hamburg;")
        .assert_success_is("Hamburg".to_string());
    // The newline is an alternative of the terminator, so the name ends there
    // and the `;` that follows is missing: a failure at the newline.
    Unframed::parse_REC()
        .parse_test("Ham\nburg;")
        .assert_failure_contains("column 4");
}

#[test]
fn inside_a_frame_the_name_ends_at_the_boundary() {
    // The newline is where the parse fails, instead of silently joining two
    // frames into one name.
    Measurements::parse_MEASUREMENT()
        .parse_test("Ham\nburg;1.0\n")
        .assert_failure_contains("column 4");
}

#[test]
fn until_still_yields_the_text_before_its_terminator() {
    Measurements::parse_MEASUREMENT()
        .parse_test("Hamburg;12.3\n")
        .assert_success_with(|(name, temp), _| {
            assert_eq!(*name, "Hamburg");
            assert_eq!(*temp, 123);
        });
}

// -----------------------------------------------------------------------------
// Split + parse + merge must agree with the sequential parse on *every* input,
// including the ones it rejects - a wrong total on some inputs and not others
// is what the checker exists to rule out.
// -----------------------------------------------------------------------------

grammar! {
    grammar Lengths {
        NAME -> &'a str = s:until(";" | frame_end) -> { s }
        #[frame]
        pub REC -> usize = n:NAME ";" "\n" -> { n.len() }
        pub FILE -> usize =
            s:par_fold(REC, || 0usize, |a: usize, v: usize| a + v, |a: usize, b: usize| a + b)
            -> { s }
    }
}

fn lengths_sequential(input: &str) -> Result<usize, String> {
    Lengths::parse_FILE()
        .parse_test(input)
        .inner
        .map_err(|e| e.to_string())
}

fn lengths_in_pieces(input: &str, n: usize) -> Result<usize, String> {
    let mut acc = Ok(0usize);
    for r in Lengths::frames_FILE(input, n) {
        let piece = Lengths::parse_FILE()
            .parse_test(&input[r])
            .inner
            .map_err(|e| e.to_string());
        acc = match (acc, piece) {
            (Ok(a), Ok(b)) => Ok(Lengths::merge_FILE(a, b)),
            (Err(e), _) | (_, Err(e)) => Err(e),
        };
    }
    acc
}

#[test]
fn pieces_agree_with_the_sequential_parse_on_awkward_input() {
    // Each of these once parsed differently in pieces than in one go, because
    // the rule's entry point skipped whitespace - once per piece rather than
    // once per input. A `par_fold` rule's parser now skips nothing at its
    // entry, so leading whitespace of a frame is part of the frame's name in
    // both, and whitespace-only garbage between frames is a failure in both.
    let inputs = [
        "A;\n B;\n  C;\n", // a frame that begins with whitespace
        " A;\nB;\n",       // the first frame does
        "A;\n  \nB;\n",    // whitespace-only garbage between frames
        "A;\nB;\n\n",      // a trailing blank line
        "A;\nB;\n  ",      // trailing spaces without a newline
        "A;\nB;",          // no trailing boundary at all
        "",
        "\n",
    ];
    for input in inputs {
        let expected = lengths_sequential(input);
        for n in [1, 2, 3, 4, 8] {
            let got = lengths_in_pieces(input, n);
            assert_eq!(
                got.is_ok(),
                expected.is_ok(),
                "{input:?} with n = {n}: sequential {expected:?}, pieces {got:?}"
            );
            if let (Ok(e), Ok(g)) = (&expected, &got) {
                assert_eq!(e, g, "{input:?} with n = {n}");
            }
        }
    }
}

// -----------------------------------------------------------------------------
// `#[frame]` after a rule without an action block
// -----------------------------------------------------------------------------

grammar! {
    grammar AttrAfterBareRule {
        // No `-> { … }` on the rule before the attribute. `#` after a rule
        // body is also how a label (`#"text"`) begins; the parser must tell
        // `#"…"` from `#[…]` rather than demand a string literal here.
        ITEM -> i32 = v:i32
        #[frame]
        pub REC -> i32 = v:ITEM "\n" -> { v }
    }
}

#[test]
fn a_frame_attribute_may_follow_a_rule_without_an_action() {
    AttrAfterBareRule::parse_REC()
        .parse_test("42\n")
        .assert_success_is(42);
}

// -----------------------------------------------------------------------------
// The driver: parse_<RULE>_pieces
// -----------------------------------------------------------------------------

use winnow_grammar::rt::Parallelism;
use winnow_grammar::ParseContext;

#[test]
fn the_driver_agrees_with_the_sequential_parse_however_it_is_run() {
    let input = sample_input(5_000);
    let expected = parse_sequential(&input);
    for how in [
        Parallelism::Off,
        Parallelism::Pieces(1),
        Parallelism::Pieces(7),
        Parallelism::Pieces(1000),
        Parallelism::Auto,
    ] {
        let got = Measurements::parse_FILE_pieces(&input, ParseContext::<()>::default, how)
            .unwrap_or_else(|e| panic!("{how:?}: {}", e.render(&input)));
        assert_eq!(got, expected, "{how:?}");
    }
}

#[test]
fn the_driver_reports_a_piece_error_at_its_offset_in_the_whole_input() {
    // Line 3 is broken. Whichever piece it lands in, the error must point at
    // line 3 of the input, not at line 1 of that piece.
    let input = "A;1.0\nB;2.0\nC;x.0\nD;4.0\nE;5.0\nF;6.0\n";
    for how in [
        Parallelism::Off,
        Parallelism::Pieces(3),
        Parallelism::Pieces(6),
    ] {
        let err = Measurements::parse_FILE_pieces(input, ParseContext::<()>::default, how)
            .expect_err("line 3 is broken");
        let rendered = err.render(input);
        assert!(
            rendered.contains("line 3"),
            "{how:?}: expected an error on line 3, got:\n{rendered}"
        );
    }
}

#[test]
fn parallelism_off_is_one_piece_and_auto_is_the_core_count() {
    assert_eq!(Parallelism::Off.pieces(), None);
    assert_eq!(Parallelism::Pieces(0).pieces(), Some(1));
    assert_eq!(Parallelism::Pieces(4).pieces(), Some(4));
    assert!(Parallelism::Auto.pieces().is_some_and(|n| n >= 1));
    assert_eq!(Parallelism::default(), Parallelism::Auto);
}

// -----------------------------------------------------------------------------
// unchecked: the author asserts the invariant
// -----------------------------------------------------------------------------

grammar! {
    grammar Asserted {
        // `until(";")` does not cover the boundary and the check would say
        // so; `unchecked` is the author saying "no name contains a newline
        // in this data". The word is there to grep for.
        NAME -> &'a str = s:until(";") -> { s }
        #[frame(boundary = "\n", unchecked)]
        pub REC -> usize = n:NAME ";" "\n" -> { n.len() }
        pub FILE -> usize =
            s:par_fold(REC, || 0usize, |a: usize, v: usize| a + v, |a: usize, b: usize| a + b)
            -> { s }
    }
}

#[test]
fn an_unchecked_frame_is_taken_at_its_word() {
    let input = "Hamburg;\nBerlin;\n";
    let seq = Asserted::parse_FILE().parse_test(input).assert_success();
    let pieces =
        Asserted::parse_FILE_pieces(input, ParseContext::<()>::default, Parallelism::Pieces(2))
            .unwrap();
    assert_eq!(seq, 13);
    assert_eq!(pieces, seq);
}

// -----------------------------------------------------------------------------
// Lookahead consumes nothing, so a rule that only `peek(…)`/`not(…)` reaches
// cannot carry the parser past the boundary. The check used to walk it anyway
// and reject a grammar that was sound.
// -----------------------------------------------------------------------------

grammar! {
    grammar Lookahead {
        WS -> () = "" -> { () }

        // Reached only through `peek`. `multispace0` would consume the
        // boundary if anything ever ran it in consuming position - nothing does.
        BLANKS -> () = multispace0 -> { () }

        // Reached only through `not`, and it names the boundary itself.
        AT_END -> &'a str = frame_end -> { "" }

        #[frame(boundary = "\n")]
        pub ROW -> usize = peek(BLANKS) not(AT_END) n:digit1 "\n" -> { n.len() }

        pub FILE -> usize = s:par_fold(
            ROW, || 0usize, |a: usize, v: usize| a + v, |a: usize, b: usize| a + b
        ) -> { s }
    }
}

#[test]
fn a_rule_only_lookahead_reaches_is_not_checked_for_the_boundary() {
    // The grammar above compiling at all is the assertion: before the walk
    // distinguished the two positions, `BLANKS` was rejected for
    // `multispace0`. `frame_end` under `not(..)` still resolves, which is why
    // the walk that resolves it still follows lookahead.
    let input = "12\n345\n6\n";
    let total = Lookahead::parse_FILE()
        .parse_test(input)
        .assert_success()
        .to_owned();
    assert_eq!(total, 6);

    for how in [
        Parallelism::Off,
        Parallelism::Pieces(2),
        Parallelism::Pieces(3),
    ] {
        let got = Lookahead::parse_FILE_pieces(input, ParseContext::<()>::default, how)
            .unwrap_or_else(|e| panic!("{how:?}: {}", e.render(input)));
        assert_eq!(got, total, "{how:?}");
    }
}

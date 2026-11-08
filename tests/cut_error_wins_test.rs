//! A cut's failure is the error (Nikaia #1413).
//!
//! `p => q`: once `p` matched, a failure in `q` is the error. A failure that
//! was only recorded while `p` was tried - an optional part of `p` that ran
//! one byte past where `p` ended - does not compete with it, however far it
//! got. The same holds for a grammar read as a stream.
//!
//! `CutEdges` pins where that ends: a cut's scope is the sequence that holds
//! it, and once that matched its records compete again. Through `#[stream]`
//! the scope can span steps - a prefix, the fold's items, a suffix - and the
//! records it sets aside can come from steps before it; each test compares
//! the stream with the parse in memory.

use std::io::Read;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseInput};

grammar! {
    grammar CutWins {
        // `name` tries `.x` after the identifier; on `!foo.y` that gets one
        // byte past the identifier's end before it backtracks. The guard
        // after the cut fails at the identifier's end.
        pub tag -> &'a str = "!" s:name => if(s == "ok", "a declared handle") -> { s }
        rule name -> &'a str = s:raw_ident ("." "x")? -> { s }

        // The same tag behind a key, so the failure is reached through a
        // sequence that is itself not committed.
        pub entry -> &'a str = _k:raw_ident ":" t:tag -> { t }

        // Entries read as a stream, the failing one after others.
        #[stream]
        pub doc -> usize = n:fold(line, || 0usize, |n: usize, _l: ()| n + 1) eof -> { n }
        line -> () = _e:entry ";" -> { () }
    }
}

#[test]
fn the_cut_failure_wins_over_a_farther_one_before_it() {
    CutWins::parse_tag()
        .parse_test("!ok")
        .assert_success_is("ok");
    CutWins::parse_tag()
        .parse_test("!foo.y")
        .assert_failure_contains("a declared handle")
        .assert_failure_not_contains("`x`");
}

#[test]
fn the_cut_failure_wins_behind_an_uncommitted_sequence() {
    CutWins::parse_entry()
        .parse_test("k:!ok")
        .assert_success_is("ok");
    CutWins::parse_entry()
        .parse_test("k:!foo.y")
        .assert_failure_contains("a declared handle")
        .assert_failure_not_contains("`x`");
}

struct Trickle<'d>(&'d [u8], usize);

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.1.min(buf.len()).min(self.0.len());
        buf[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

/// Small blocks: every read is a refill.
const SMALL: Reading = Reading {
    block: 16,
    max_frame: 256,
};

#[test]
fn the_cut_failure_wins_in_a_stream_as_in_memory() {
    let input = "a:!ok;bb:!ok;c:!foo.y;";
    let at = input.find("foo").unwrap() + "foo".len();
    for mode in [Diagnose::Replay, Diagnose::ReplayInPlace, Diagnose::Eager] {
        let mem = CutWins::parse_doc()
            .parse_next(&mut ParseInput {
                input: LocatingSlice::new(input),
                state: ParseContext::<()> {
                    diagnose: mode,
                    ..Default::default()
                },
            })
            .unwrap_err();
        assert_eq!(mem.offset, at, "{mode:?} in memory: {mem:?}");
        assert!(
            mem.to_string().contains("a declared handle"),
            "{mode:?} in memory: {mem}"
        );
        for step in [1, 2, 3, 1 << 20] {
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            let got = CutWins::parse_doc_reader(Trickle(input.as_bytes(), step), &mut ctx, SMALL)
                .unwrap_err();
            assert_eq!(got.offset, mem.offset, "{mode:?} step {step}: {got:?}");
            assert_eq!(got.headline(), mem.headline(), "{mode:?} step {step}");
        }
    }
}

// --- Edge cases: when the records from before a cut come back, and the
// stream agreeing with the parse in memory across its steps.

grammar! {
    grammar CutEdges {
        // The cut's scope ends with the sequence that holds it: once `tag`
        // matched, a failure after it backtracks past the committed part,
        // and what was recorded before the cut competes again. On `!ok.y)`
        // that is `name`'s `.x` attempt, one byte past where `)` failed.
        pub closed -> &'a str = t:tag ")" -> { t }
        rule tag -> &'a str = "!" s:name => if(s == "ok", "a declared handle") -> { s }
        rule name -> &'a str = s:raw_ident ("." "x")? -> { s }

        // The stream's prefix records farther than an item's cut fails: in
        // memory the cut set that record aside, so the stream must too,
        // although prefix and item are parsed as separate steps.
        #[stream]
        pub far -> usize = _p:far_prefix n:fold(item, || 0usize, |n: usize, _i: ()| n + 1) eof -> { n }
        far_prefix -> () = ("!" "q" ";" "zz")? -> { () }
        item -> () = "!" => "q" "w" ";" -> { () }

        // A cut in the stream rule's own prefix: its scope runs through
        // the fold to the end of the rule. What was recorded before it does
        // not compete with a cut failure in an item, and competes again
        // once the rule matched - here with the input left over.
        #[stream]
        pub open -> usize = "%" _o:abc => n:fold(hash, || 0usize, |n: usize, _h: ()| n + 1) -> { n }
        abc -> () = ("#" "y" "c" | "@" "b" "c")? -> { () }
        hash -> () = "#" => "y" ";" -> { () }

        // Two rules on the stream's path, each with a cut before the rule it
        // continues in. `lvl`'s scope ends with its `)`; `reach`, before
        // its cut, wanted `c` at the offset where `nest`'s `;` then fails -
        // a cut failure, which must see `c` again.
        #[stream]
        pub nest -> usize = "%" => n:lvl ";" -> { n }
        lvl -> usize = "(" _o:reach => n:fold(hash, || 0usize, |n: usize, _h: ()| n + 1) ")" -> { n }
        reach -> () = ("#" "y" ";" ")" "c")? -> { () }

        // The rule the path continues in has no cut, its caller has one
        // before the call: `)` failing in `plain` is a cut failure in
        // memory (the call is after the cut), and `z`, recorded farther
        // before that cut, does not compete - in the stream as well, where
        // `plain`'s suffix is a step of its own.
        #[stream]
        pub outer -> usize = "%" _r:far0 => n:plain ";" -> { n }
        far0 -> () = ("(" "#" "y" ";" "]" "z")? -> { () }
        plain -> usize = "(" n:fold(hash, || 0usize, |n: usize, _h: ()| n + 1) ")" -> { n }

        // A suffix with a cut of its own, read a byte at a time: its first
        // tries stop at the end of the window after the cut and are parsed
        // again over more.
        #[stream]
        pub retry -> usize = n:fold(hash, || 0usize, |n: usize, _h: ()| n + 1) _s:bang -> { n }
        bang -> () = "!" => "a" "b" "c" "d" "e" "f" "g" "h" "i" "j" -> { () }
    }
}

#[test]
fn records_before_a_cut_compete_again_once_its_sequence_matched() {
    CutEdges::parse_closed()
        .parse_test("!ok)")
        .assert_success_is("ok");
    // `)` fails at offset 3; `.x` was tried up to offset 4 before the cut.
    CutEdges::parse_closed()
        .parse_test("!ok.y)")
        .assert_failure_contains("`x`");
    // And the cut failure itself still wins inside it.
    CutEdges::parse_closed()
        .parse_test("!foo.y)")
        .assert_failure_contains("a declared handle")
        .assert_failure_not_contains("`x`");
}

const EVERY_MODE: &[Diagnose] = &[Diagnose::Replay, Diagnose::ReplayInPlace, Diagnose::Eager];

/// The stream's answer for `input` under `modes` and every read size,
/// against the parse in memory: the same offset, the same headline.
fn stream_agrees<F, G>(
    input: &str,
    modes: &[Diagnose],
    mem_parse: F,
    reader_parse: G,
) -> Vec<winnow_grammar::ParseError>
where
    F: Fn(&mut ParseInput<'_, ()>) -> Result<usize, winnow_grammar::ParseError>,
    G: Fn(Trickle<'_>, &mut ParseContext<()>) -> Result<usize, winnow_grammar::ParseError>,
{
    let mut out = Vec::new();
    for &mode in modes {
        let mem = mem_parse(&mut ParseInput {
            input: LocatingSlice::new(input),
            state: ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            },
        })
        .unwrap_err();
        for step in [1, 2, 3, 1 << 20] {
            let mut ctx = ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            };
            let got = reader_parse(Trickle(input.as_bytes(), step), &mut ctx).unwrap_err();
            assert_eq!(
                (got.offset, got.headline()),
                (mem.offset, mem.headline()),
                "{input:?} {mode:?} step {step}: stream {got:?}\nmemory {mem:?}"
            );
        }
        out.push(mem);
    }
    out
}

#[test]
fn a_record_from_an_earlier_step_does_not_beat_a_cut_failure_in_a_stream() {
    // `far_prefix` gets to offset 3 (wanting `zz`) and backtracks; the item
    // commits after `!` and fails at offset 2 wanting `w`.
    for mem in stream_agrees(
        "!q;",
        EVERY_MODE,
        |i| CutEdges::parse_far().parse_next(i),
        |r, c| CutEdges::parse_far_reader(r, c, SMALL),
    ) {
        assert_eq!(mem.offset, 2, "{mem:?}");
        assert!(mem.to_string().contains("`w`"), "{mem}");
        assert!(!mem.to_string().contains("`zz`"), "{mem}");
    }
}

#[test]
fn a_cut_in_the_stream_prefix_holds_through_the_fold() {
    // A cut failure in an item at offset 3, where `abc`, before the
    // prefix's cut, wanted `c`: it does not compete, not even at the same
    // offset.
    for mem in stream_agrees(
        "%#y!",
        EVERY_MODE,
        |i| CutEdges::parse_open().parse_next(i),
        |r, c| CutEdges::parse_open_reader(r, c, SMALL),
    ) {
        assert_eq!(mem.offset, 3, "{mem:?}");
        assert!(mem.to_string().contains("`;`"), "{mem}");
        assert!(!mem.to_string().contains("`c`"), "{mem}");
    }
    // The rule matched (no items: `@` begins none) and input is left over
    // at offset 1: the scope is over, so `c` at offset 3 is the better
    // message again.
    for mem in stream_agrees(
        "%@bX",
        EVERY_MODE,
        |i| CutEdges::parse_open().parse_next(i),
        |r, c| CutEdges::parse_open_reader(r, c, SMALL),
    ) {
        assert_eq!(mem.offset, 3, "{mem:?}");
        assert!(mem.to_string().contains("`c`"), "{mem}");
    }
}

#[test]
fn a_cut_scope_spanning_steps_ends_with_its_rule() {
    CutEdges::parse_nest()
        .parse_test("%(#y;);")
        .assert_success_is(1);
    // `c` and `;` both at offset 6: once `lvl` matched, `c` competes again,
    // with the cut failure of `nest`.
    for mem in stream_agrees(
        "%(#y;)X",
        EVERY_MODE,
        |i| CutEdges::parse_nest().parse_next(i),
        |r, c| CutEdges::parse_nest_reader(r, c, SMALL),
    ) {
        assert_eq!(mem.offset, 6, "{mem:?}");
        assert!(mem.to_string().contains("`c`"), "{mem}");
        assert!(mem.to_string().contains("`;`"), "{mem}");
    }
}

#[test]
fn a_failure_under_an_open_cut_scope_is_a_cut_failure_in_a_stream() {
    CutEdges::parse_outer()
        .parse_test("%(#y;);")
        .assert_success_is(1);
    // `)` fails at offset 5; `z` was wanted at offset 6 before the cut.
    for mem in stream_agrees(
        "%(#y;]",
        EVERY_MODE,
        |i| CutEdges::parse_outer().parse_next(i),
        |r, c| CutEdges::parse_outer_reader(r, c, SMALL),
    ) {
        assert_eq!(mem.offset, 5, "{mem:?}");
        assert!(mem.to_string().contains("`)`"), "{mem}");
        assert!(!mem.to_string().contains("`z`"), "{mem}");
    }
}

/// One-byte blocks: a step sees the input as it trickles in.
const TINY: Reading = Reading {
    block: 1,
    max_frame: 256,
};

#[test]
fn a_step_parsed_again_gives_back_what_its_cut_set_aside() {
    // What a try that stopped at the end of the window set aside is given
    // back before the step is parsed again: no cut is left open at the end
    // (`Stream::finish` asserts it in a debug build).
    for mode in EVERY_MODE {
        for step in [1, 2, 3, 1 << 20] {
            let mut ctx = ParseContext::<()> {
                diagnose: *mode,
                ..Default::default()
            };
            let n =
                CutEdges::parse_retry_reader(Trickle(b"#y;#y;!abcdefghij", step), &mut ctx, TINY)
                    .unwrap();
            assert_eq!(n, 2, "{mode:?} step {step}");
        }
    }
    // And a failure after the cut, once all of it has arrived.
    stream_agrees(
        "#y;!abcdefgX",
        EVERY_MODE,
        |i| CutEdges::parse_retry().parse_next(i),
        |r, c| CutEdges::parse_retry_reader(r, c, TINY),
    );
}

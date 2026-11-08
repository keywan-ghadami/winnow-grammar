//! A cut's failure is the error (Nikaia #1413).
//!
//! `p => q`: once `p` matched, a failure in `q` is the error. A failure that
//! was only recorded while `p` was tried - an optional part of `p` that ran
//! one byte past where `p` ended - does not compete with it, however far it
//! got. The same holds for a grammar read as a stream.

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

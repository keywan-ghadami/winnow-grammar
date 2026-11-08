//! What a stream's prefix and suffix record, under every `Diagnose` mode.
//!
//! The parse in memory replays a failed fast pass from the start, so an
//! attempt given up inside a part that matched - an option before the fold,
//! one after it - is there for an error further on. A stream does not go
//! back that far: under `Replay` and `ReplayInPlace` a prefix or suffix that
//! the fast pass matched is parsed again by the diagnosing pass right away,
//! for its records only. Its value and its effects on the user state are
//! the fast pass's.

use std::io::Read;
use winnow::stream::LocatingSlice;
use winnow::Parser;
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, Diagnose, ParseContext, ParseError, ParseInput};

grammar! {
    grammar Steps {
        // `abc` gets to offset 3 on `%@bX` and backtracks; no item begins
        // with `@`, and the input left over at offset 1 is the failure.
        #[stream]
        pub head -> usize = "%" _o:abc n:fold(hash, || 0usize, |n: usize, _h: ()| n + 1) -> { n }
        abc -> () = ("@" "b" "c")? -> { () }
        hash -> () = "#" "y" ";" -> { () }

        // The same after the fold: `qr` gets to offset 5 on `#y;!qX`.
        #[stream]
        pub tail -> usize = n:fold(hash, || 0usize, |n: usize, _h: ()| n + 1) _t:qr -> { n }
        qr -> () = ("!" "q" "r")? -> { () }
    }
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

const SMALL: Reading = Reading {
    block: 16,
    max_frame: 256,
};

const MODES: [Diagnose; 3] = [Diagnose::Replay, Diagnose::ReplayInPlace, Diagnose::Eager];

fn agree<M, R>(input: &str, mem_parse: M, reader_parse: R) -> ParseError
where
    M: Fn(&mut ParseInput<'_, ()>) -> Result<usize, ParseError>,
    R: Fn(Trickle<'_>, &mut ParseContext<()>) -> Result<usize, ParseError>,
{
    let mut last = None;
    for mode in MODES {
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
        last = Some(mem);
    }
    last.unwrap()
}

#[test]
fn what_a_prefix_gave_up_is_there_for_an_error_further_on() {
    let mem = agree(
        "%@bX",
        |i| Steps::parse_head().parse_next(i),
        |r, c| Steps::parse_head_reader(r, c, SMALL),
    );
    assert_eq!(mem.offset, 3, "{mem:?}");
    assert!(mem.to_string().contains("`c`"), "{mem}");
}

#[test]
fn what_a_suffix_gave_up_is_there_for_an_error_further_on() {
    let mem = agree(
        "#y;!qX",
        |i| Steps::parse_tail().parse_next(i),
        |r, c| Steps::parse_tail_reader(r, c, SMALL),
    );
    assert_eq!(mem.offset, 5, "{mem:?}");
    assert!(mem.to_string().contains("`r`"), "{mem}");
}

// --- The replay of a step leaves the user state as the fast pass left it --

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    pub words: Vec<String>,
}

grammar! {
    grammar Noted {
        use super::Seen;
        state Seen;

        #[stream]
        pub doc -> usize = _h:title n:fold(word, || 0usize, |n: usize, _w: ()| n + 1) _e:end -> { n }
        title -> () = "%" t:raw_ident -> { _state.user().words.push(format!("title {t}")); }
        word -> () = "#" w:raw_ident -> { _state.user().words.push(w.to_string()); }
        end -> () = "." -> { _state.user().words.push("end".to_string()); }
    }
}

#[test]
fn a_replayed_step_runs_its_actions_for_nothing() {
    let input = "%t #a #b .";
    for mode in MODES {
        for step in [1, 3, 1 << 20] {
            let mut ctx = ParseContext::with_state(Seen::default());
            ctx.diagnose = mode;
            let n =
                Noted::parse_doc_reader(Trickle(input.as_bytes(), step), &mut ctx, SMALL).unwrap();
            assert_eq!(n, 2);
            assert_eq!(
                ctx.user_state.words,
                ["title t", "a", "b", "end"],
                "{mode:?} step {step}"
            );
        }
    }
}

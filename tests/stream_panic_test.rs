//! A stream parses on the caller's context: the item loop moves it out for a
//! window and back (`rt::stream::Hold`). A panic in an action while it is out
//! must not leave the caller's context moved-from - it is written back as
//! the panic unwinds, and the context can be dropped and used again.

use std::panic::{catch_unwind, AssertUnwindSafe};
use winnow_grammar::rt::Reading;
use winnow_grammar::{grammar, ParseContext};

#[derive(Clone, Debug, Default, PartialEq)]
struct Seen {
    items: Vec<i64>,
}

grammar! {
    grammar Panicky {
        state Seen;

        #[stream]
        pub list -> usize = "[" n:fold(num, || 0usize, |n: usize, _v: i64| n + 1) "]" -> { n }
        num -> i64 = v:i64 ","? -> {
            assert!(v != 13, "thirteen");
            _state.user().items.push(v);
            v
        }
    }
}

fn reading() -> Reading {
    Reading {
        block: 4,
        max_frame: 64,
    }
}

#[test]
fn a_panic_in_the_item_loop_leaves_the_context_usable() {
    let mut ctx = ParseContext::<Seen>::default();
    let r = catch_unwind(AssertUnwindSafe(|| {
        Panicky::parse_list_slice(b"[1, 2, 13, 4]", &mut ctx)
    }));
    assert!(r.is_err(), "the action panicked");
    // What the items before the panic did is there; the context is whole.
    assert_eq!(ctx.user_state.items, vec![1, 2]);
    ctx.user_state.items.clear();
    let n = Panicky::parse_list_slice(b"[5, 6]", &mut ctx).unwrap();
    assert_eq!(n, 2);
    assert_eq!(ctx.user_state.items, vec![5, 6]);
}

#[test]
fn a_panic_in_the_reader_leaves_the_context_usable() {
    let mut ctx = ParseContext::<Seen>::default();
    let r = catch_unwind(AssertUnwindSafe(|| {
        Panicky::parse_list_reader(std::io::Cursor::new("[1, 2, 13, 4]"), &mut ctx, reading())
    }));
    assert!(r.is_err(), "the action panicked");
    ctx.user_state.items.clear();
    let n =
        Panicky::parse_list_reader(std::io::Cursor::new("[7, 8, 9]"), &mut ctx, reading()).unwrap();
    assert_eq!(n, 3);
    assert_eq!(ctx.user_state.items, vec![7, 8, 9]);
    drop(ctx);
}

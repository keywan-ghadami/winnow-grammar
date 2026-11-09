//! A rule's visibility (`pub`, `pub(crate)`, `pub(super)`) goes to every
//! function generated for it: the parser, `_reader` and `_reader_each`
//! (#1464 of nikaia). Here a `pub(crate)` grammar is used from outside its
//! module; `tests/ui/visibility.rs` shows the same functions refused where
//! the visibility does not reach.

use std::ops::ControlFlow;
use winnow_grammar::rt::Reading;
use winnow_grammar::testing::WinnowTestExt;
use winnow_grammar::ParseContext;

mod inner {
    winnow_grammar::grammar! {
        grammar Vis {
            #[stream]
            pub(crate) count -> usize =
                n:fold(item, || 0usize, |n: usize, _i: i64| n + 1) eof -> { n }

            #[stream(each)]
            pub(crate) each -> usize =
                n:fold(item, || 0usize, |n: usize, _i: i64| n + 1) eof -> { n }

            pub(crate) one -> i64 = i:item -> { i }

            item -> i64 = n:i64 -> { n }
        }
    }
}

#[test]
fn a_crate_visible_rule_is_parsed_from_outside_its_module() {
    inner::Vis::parse_one()
        .parse_test("42")
        .assert_success_is(42);
}

#[test]
fn a_crate_visible_stream_rule_is_read_from_outside_its_module() {
    let n = inner::Vis::parse_count_reader(
        std::io::Cursor::new("1 2 3"),
        &mut ParseContext::<()>::default(),
        Reading::default(),
    );
    assert_eq!(n.unwrap(), 3);

    let mut seen = Vec::new();
    let n = inner::Vis::parse_each_reader_each(
        std::io::Cursor::new("4 5"),
        &mut ParseContext::<()>::default(),
        Reading::default(),
        |&i: &i64| {
            seen.push(i);
            ControlFlow::<()>::Continue(())
        },
    );
    assert!(matches!(n, Ok(ControlFlow::Continue(2))));
    assert_eq!(seen, [4, 5]);
}

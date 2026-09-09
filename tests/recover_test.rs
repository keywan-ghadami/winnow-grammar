use winnow_grammar::grammar;
use winnow_grammar::testing::WinnowTestExt;

grammar! {
    grammar RecoverTest {
        rule item -> i32 = i:i32 ";" -> { i }

        pub rule list -> Vec<Option<i32>> =
            items:recover(item, ";")* -> { items }
    }
}

#[test]
fn test_recovery() {
    RecoverTest::parse_list()
        .parse_test("1; 2; bad; 3;")
        .assert_success_is(vec![Some(1), Some(2), None, Some(3)]);
}

// -----------------------------------------------------------------------------
// What a recovery swallowed.
//
// `recover(rule, sync)` used to discard the failure entirely: the parse
// carried on and nothing said what had been wrong. The count is now kept in
// both passes, and the error itself wherever the diagnosing engine is the one
// running.
// -----------------------------------------------------------------------------

mod recovered {
    use winnow::stream::LocatingSlice;
    use winnow::Parser;
    use winnow_grammar::{grammar, Diagnose, ParseContext, ParseInput};

    grammar! {
        grammar Items {
            WS -> () = "" -> { () }
            // The separator belongs to the item, so that recovering to it
            // consumes it exactly once.
            rule item -> u32 = v:u32 ";" -> { v }
            pub list -> usize = xs:recover(item, ";")* -> {
                xs.iter().filter(|x| x.is_some()).count()
            }
        }
    }

    fn run(mode: Diagnose, input: &str) -> (usize, ParseContext<()>) {
        let mut stream = ParseInput {
            input: LocatingSlice::new(input),
            state: ParseContext::<()> {
                diagnose: mode,
                ..Default::default()
            },
        };
        let n = Items::parse_list().parse_next(&mut stream).unwrap();
        (n, stream.state)
    }

    #[test]
    fn a_clean_parse_recovers_nothing() {
        let (n, ctx) = run(Diagnose::Replay, "1;2;3;");
        assert_eq!(n, 3);
        assert_eq!(ctx.recoveries, 0);
        assert!(ctx.recovered.is_empty());
    }

    #[test]
    fn the_count_survives_the_fast_pass() {
        // The default mode: a parse that recovers still *succeeds*, so only
        // the fast pass runs and there is no error object to keep. The count
        // is what tells the caller to look closer.
        let (n, ctx) = run(Diagnose::Replay, "1;x;3;y;");
        assert_eq!(n, 2, "two items parsed, two skipped");
        assert_eq!(ctx.recoveries, 2);
        assert!(
            ctx.recovered.is_empty(),
            "the fast pass has no error to record"
        );
    }

    #[test]
    fn diagnosing_says_what_was_wrong_and_where() {
        let (n, ctx) = run(Diagnose::Eager, "1;x;3;y;");
        assert_eq!(n, 2);
        assert_eq!(ctx.recoveries, 2);
        assert_eq!(ctx.recovered.len(), 2, "one error per recovery");

        let first = ctx.recovered[0].render("1;x;3;y;");
        assert!(
            first.contains("integer literal"),
            "the error is the item's own: {first}"
        );
        // The two are at different places, and in the order they happened.
        assert!(ctx.recovered[0].offset < ctx.recovered[1].offset);
    }

    #[test]
    fn a_reused_context_does_not_accumulate_recoveries() {
        // `begin_parse` owns these, like the rest of the engine's workspace.
        let (_, ctx) = run(Diagnose::Eager, "1;x;");
        assert_eq!(ctx.recoveries, 1);
        let (_, ctx2) = run(Diagnose::Eager, "1;x;");
        assert_eq!(ctx2.recoveries, 1, "not two");
    }
}

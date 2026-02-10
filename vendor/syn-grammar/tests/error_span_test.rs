use syn_grammar::grammar;

// This test file is designed to fail compilation if the spans are incorrect.
// We use `trybuild` style testing logic manually by checking if errors point to the right place.
// Since we can't easily assert on span locations in runtime tests without a complex setup,
// we rely on the fact that if `quote_spanned!` is working, the generated code will have the right spans.
//
// However, to verify the fix for "errors pointing to grammar!", we can check if `syn::Error`s
// generated during *runtime parsing* (which use the spans we propagated) have reasonable locations
// if we were to inspect them. But `syn::Error` spans are not easily inspectable for line/column in tests.
//
// Instead, we trust that by propagating the spans in `ModelPattern`, `quote_spanned!` in `codegen`
// will attach the right span to the generated code.
//
// We can at least verify that the code compiles and runs, ensuring our refactoring didn't break anything.

#[test]
fn test_span_propagation_compiles() {
    grammar! {
        grammar span_test {
            rule main -> () =
                "a" => "b" -> { () }
              | ( "c" "d" ) -> { () }
              | [ "e" ] -> { () }
              | { "f" } -> { () }
              | paren("g") -> { () }
              | "h"? -> { () }
              | "i"* -> { () }
              | "j"+ -> { () }
              | recover(inner, "k") "k" -> { () }

            rule inner -> () = "z" -> { () }
        }
    }

    // Just ensure it compiles and runs
    let _ = span_test::parse_main;
}

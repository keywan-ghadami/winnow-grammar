use winnow_grammar::grammar;

// Test case: Unreachable Alternative (Should Warn or Fail)
grammar! {
    grammar Ambiguous {
        rule choice -> () =
            "a" -> { () }
          | "a" -> { () } // Unreachable
    }
}

fn main() {}

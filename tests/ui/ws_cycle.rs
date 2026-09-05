use winnow_grammar::grammar;

// `comment` is lowercase, hence syntactic: it calls `WS` at its start, and
// `WS` calls it. Previously a stack overflow at runtime without any
// diagnostic; now a message at macro time that points at `comment`.
grammar! {
    grammar Test {
        WSE = multispace1
        WS = (WSE | comment)*
        comment = "//" until(line_ending)

        pub add -> i32 = a:i32 "+" b:i32 -> { a + b }
    }
}

fn main() {}

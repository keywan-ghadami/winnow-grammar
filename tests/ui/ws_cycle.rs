use winnow_grammar::grammar;

// `comment` ist kleingeschrieben, also syntaktisch: es ruft an seinem Anfang
// `WS`, und `WS` ruft es. Vorher ein Stack-Overflow zur Laufzeit ohne jede
// Diagnose; jetzt eine Meldung zur Makro-Zeit, die auf `comment` zeigt.
grammar! {
    grammar Test {
        WSE = multispace1
        WS = (WSE | comment)*
        comment = "//" until(line_ending)

        pub add -> i32 = a:i32 "+" b:i32 -> { a + b }
    }
}

fn main() {}

use winnow::prelude::*;
// use winnow_grammar::grammar;

// -----------------------------------------------------------------------------
// 1. AST (Datenstruktur)
// -----------------------------------------------------------------------------

#[derive(Debug, PartialEq, Clone)]
pub enum Field {
    Any,                   // *
    Number(u32),           // 15
    Range(u32, u32),       // 1-5
    List(Vec<Field>),      // 1,2,3
    Step(Box<Field>, u32), // */5
}

#[derive(Debug, PartialEq)]
pub struct CronSchedule {
    pub sec: Field,
    pub min: Field,
    pub hour: Field,
    pub dom: Field,
    pub month: Field,
    pub dow: Field,
}

// -----------------------------------------------------------------------------
// 2. Grammatik (Clean Version - Auto Whitespace)
// -----------------------------------------------------------------------------

winnow_grammar::grammar! {
    grammar Cron {
        // HIER DER UNTERSCHIED: Keine `_` Regeln mehr nötig.
        // Das Makro kümmert sich um "0 * * ..."
        pub rule schedule -> CronSchedule =
            s:field m:field h:field dom:field mon:field dow:field
            -> {
                CronSchedule { sec: s, min: m, hour: h, dom: dom, month: mon, dow: dow }
            }

        // --- Precedence Layering ---

        // 1. Listen: "1, 2, 3" (Leerzeichen nach Komma werden ignoriert)
        rule field -> Field =
            head:step_expr tail:comma_step* -> {
                if tail.is_empty() {
                    head
                } else {
                    let mut list = vec![head];
                    list.extend(tail);
                    Field::List(list)
                }
            }

        rule comma_step -> Field =
            "," s:step_expr -> { s }

        // 2. Steps: "*/5" oder "10 - 20 / 2" (Whitespace um / erlaubt)
        rule step_expr -> Field =
            base:range_expr step:slash_int? -> {
                match step {
                    Some(s) => Field::Step(Box::new(base), s),
                    None => base,
                }
            }

        rule slash_int -> u32 =
            "/" n:uint -> { n }

        // 3. Ranges: "1-5"
        rule range_expr -> Field =
            start:atom end:dash_atom? -> {
                match end {
                    Some(e) => {
                        // Simpler Check im Action Block
                        match (start, e) {
                            (Field::Number(s), Field::Number(e)) => Field::Range(s, e),
                            _ => panic!("Range syntax requires numbers"),
                        }
                    },
                    None => start,
                }
            }

        rule dash_atom -> Field =
            "-" a:atom -> { a }

        // 4. Atome
        rule atom -> Field =
            "*"       -> { Field::Any }
          | n:uint -> { Field::Number(n) }
    }
}

// -----------------------------------------------------------------------------
// 3. Tests
// -----------------------------------------------------------------------------

#[test]
fn test_standard_cron() {
    let mut input = "0 30 9 * * *"; // Standard Unix Style
    let result = Cron::parse_schedule.parse(&mut input).unwrap();

    assert_eq!(result.min, Field::Number(30));
    assert_eq!(result.hour, Field::Number(9));
}

#[test]
fn test_messy_whitespace() {
    // DAS ist der Beweis, dass dein Auto-Whitespace funktioniert.
    // Wir nutzen Leerzeichen an Stellen, wo Standard-Cron strikt wäre,
    // aber Token-Parser oft lax sind (was gut für UX ist).

    // "0" Space "1 , 2" Space "*/ 15" ...
    let mut input = "0   1 , 2   */ 15   * * *";

    let result = Cron::parse_schedule
        .parse(&mut input)
        .expect("Should handle messy whitespace");

    // Check List parsing mit spaces "1 , 2"
    if let Field::List(items) = result.min {
        assert_eq!(items, vec![Field::Number(1), Field::Number(2)]);
    } else {
        panic!("Failed to parse list with spaces");
    }

    // Check Step parsing mit spaces "*/ 15"
    if let Field::Step(base, step) = result.hour {
        assert_eq!(*base, Field::Any);
        assert_eq!(step, 15);
    } else {
        panic!("Failed to parse step with spaces");
    }
}

#[test]
fn test_complex_precedence() {
    // 10-20/2 -> Step bindet schwächer als Range
    let mut input = "0 10-20/2 * * * *";
    let result = Cron::parse_schedule.parse(&mut input).unwrap();

    assert_eq!(result.min, Field::Step(Box::new(Field::Range(10, 20)), 2));
}

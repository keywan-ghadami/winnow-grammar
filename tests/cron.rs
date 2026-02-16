use winnow::prelude::*;
use winnow::stream::LocatingSlice;
use winnow_grammar::grammar;

#[derive(Debug, PartialEq)]
pub struct Schedule {
    pub second: Field,
    pub minute: Field,
    pub hour: Field,
    pub dom: Field,
    pub month: Field,
    pub dow: Field,
}

#[derive(Debug, PartialEq)]
pub enum Field {
    Any,
    Value(u32),
    Range(u32, u32),
    List(Vec<Field>),
    Step(Box<Field>, u32),
}

grammar! {
    grammar Cron {
        pub rule schedule -> Schedule =
            sec:field min:field hour:field dom:field mon:field dow:field -> {
                Schedule {
                    second: sec,
                    minute: min,
                    hour: hour,
                    dom: dom,
                    month: mon,
                    dow: dow,
                }
            }

        rule field -> Field =
            l:list -> { if l.len() == 1 { l.into_iter().next().unwrap() } else { Field::List(l) } }

        rule list -> Vec<Field> =
            base:base_field "," rest:list -> { let mut rest = rest; rest.insert(0, base); rest }
          | base:base_field -> { vec![base] }

        rule base_field -> Field =
            f:range_or_val s:step? -> {
                match s {
                    Some(step) => Field::Step(Box::new(f), step),
                    None => f,
                }
            }
          | "*" s:step? -> {
                match s {
                    Some(step) => Field::Step(Box::new(Field::Any), step),
                    None => Field::Any,
                }
            }

        rule range_or_val -> Field =
            a:u32 "-" b:u32 -> { Field::Range(a, b) }
          | v:u32 -> { Field::Value(v) }

        rule step -> u32 =
            "/" n:u32 -> { n }
    }
}

#[test]
fn test_standard_cron() {
    let input = LocatingSlice::new("0 30 9 * * 1-5");
    let result = Cron::parse_schedule.parse(input).unwrap();
    assert_eq!(
        result,
        Schedule {
            second: Field::Value(0),
            minute: Field::Value(30),
            hour: Field::Value(9),
            dom: Field::Any,
            month: Field::Any,
            dow: Field::Range(1, 5),
        }
    );
}

#[test]
fn test_complex_precedence() {
    // "*/5" -> Step(Any, 5)
    // "1-10/2" -> Step(Range(1, 10), 2)
    // "1,2,3" -> List(...)
    let input = LocatingSlice::new("*/5 1-10/2 1,2,3 * * *");
    let result = Cron::parse_schedule.parse(input).unwrap();

    match result.second {
        Field::Step(f, 5) => assert_eq!(*f, Field::Any),
        _ => panic!("Expected */5"),
    }

    match result.minute {
        Field::Step(f, 2) => assert_eq!(*f, Field::Range(1, 10)),
        _ => panic!("Expected 1-10/2"),
    }

    match result.hour {
        Field::List(l) => assert_eq!(l.len(), 3),
        _ => panic!("Expected list"),
    }
}

#[test]
fn test_messy_whitespace() {
    let input = LocatingSlice::new(" 0   30\t9 * \n * 1-5");
    let result = Cron::parse_schedule.parse(input).unwrap();
    assert_eq!(result.second, Field::Value(0));
    assert_eq!(result.minute, Field::Value(30));
}

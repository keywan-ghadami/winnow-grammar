# winnow-grammar

[![Crates.io](https://img.shields.io/crates/v/winnow-grammar.svg)](https://crates.io/crates/winnow-grammar)
[![Documentation](https://docs.rs/winnow-grammar/badge.svg)](https://docs.rs/winnow-grammar)
[![License](https://img.shields.io/crates/l/winnow-grammar.svg)](https://github.com/keywan-ghadami/winnow-grammar/blob/main/LICENSE)

**winnow-grammar** is a powerful parser generator for Rust that allows you to define EBNF-like grammars directly inside your code. It compiles these definitions into efficient `winnow` parsers at compile time.

This crate is built on top of `syn-grammar-model` but targets the `winnow` parser combinator library. While `syn-grammar` is specialized for parsing Rust code, `winnow-grammar` is designed for general-purpose parsing of text, data formats, and custom DSLs (using `&str` or `&[u8]`).

## Documentation

- **[Grammar Syntax Reference](../SYNTAX.md)**: Detailed guide to the shared grammar definition language (rules, operators, built-ins).
- **[Extending Guide](../EXTENDING.md)**: Guide for library authors on how to build custom backends.

## Features

- **Inline Grammars**: Define your grammar directly in your Rust code using the `grammar!` macro.
- **Type-Safe Actions**: Directly map parsing rules to Rust types and AST nodes using action blocks (`-> { ... }`).
- **Winnow Integration**: Generates efficient `winnow` parsers that work with standard `winnow` traits.
- **Whitespace Handling**: Automatic whitespace skipping (configurable).
- **Span Tracking**: Support for `LocatingSlice` to track source positions (e.g., `rule @ span`).

## Installation

Add `winnow-grammar` and `winnow` to your `Cargo.toml`.

```toml
[dependencies]
winnow-grammar = "0.1.0"
winnow = "0.6"
```

## Quick Start

Here is a complete example of a Cron expression parser.

```rust
use winnow_grammar::grammar;
use winnow::prelude::*;
use winnow::stream::LocatingSlice;

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
        pub schedule -> Schedule =
            sec:field min:field hour:field dom:field mon:field dow:field -> {
                Schedule {
                    second: sec,
                    minute: min,
                    hour,
                    dom,
                    month: mon,
                    dow,
                }
            }

        field -> Field =
            l:list -> { if l.len() == 1 { l.into_iter().next().unwrap() } else { Field::List(l) } }

        list -> Vec<Field> =
            base:base_field "," rest:list -> { let mut rest = rest; rest.insert(0, base); rest }
          | base:base_field -> { vec![base] }

        base_field -> Field =
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

        range_or_val -> Field =
            a:u32 "-" b:u32 -> { Field::Range(a, b) }
          | v:u32 -> { Field::Value(v) }

        step -> u32 =
            "/" n:u32 -> { n }
    }
}

fn main() {
    let input = "0 30 9 * * 1-5";
    // We use LocatingSlice to support span tracking if needed.
    let input = LocatingSlice::new(input);
    
    let result = Cron::parse_schedule.parse(input);
    println!("{:?}", result);
}
```

### What happens under the hood?

The `grammar!` macro expands into a Rust module containing:
- A function `parse_<rule_name>` for each rule.
- These functions take a `&mut I` where `I` is a `winnow` stream (e.g., `&str`, `LocatingSlice<&str>`).

## Backend Specifics

### Input Type
The generated parsers work on any input that implements the necessary `winnow` traits (`&str`, `&[u8]`).

If you use **Span Binding (`@`)**, your input type **must** implement `winnow::stream::Location`. The recommended type for this is `winnow::stream::LocatingSlice`.

### Whitespace Handling
By default, `winnow-grammar` automatically skips whitespace between tokens in syntactic rules (rules with lowercase names). The default whitespace parser is equivalent to `winnow::ascii::multispace0`.

You can override this behavior by defining a special rule named `ws`. This is a powerful feature for handling more complex spacing, like comments. For example, to make your parser treat `//` style comments as whitespace:

```rust
use winnow_grammar::grammar;
use winnow::prelude::*;
use winnow::stream::LocatingSlice;

grammar! {
    grammar CommentAware {
        // Override 'ws' to skip spaces, newlines, and single-line comments.
        WSE = multispace1
        WS = (WSE | comment)*

        // A rule that recognizes a single-line comment.
        // `line_ending` is a built-in parser. `until` consumes input up to it.
        comment = "//" until(line_ending)

        // This rule can now have comments between its tokens because it's a
        // syntactic rule (lowercase name).
        pub add -> i32 =
            a:i32
            // This is a comment, which our `ws` rule will now handle!
            "+"
            b:i32
            -> { a + b }
    }
}

fn main() {
    // The parser will ignore the comment and the newline.
    let input = "10 // add 20
 + 20";
    let stream = LocatingSlice::new(input);
    let result = CommentAware::parse_add.parse(stream).unwrap();
    assert_eq!(result, 30);
}
```

### Built-ins
In addition to the portable built-ins (see [SYNTAX.md](../SYNTAX.md)), `winnow-grammar` provides the following `winnow`-specific parsers:

| Parser | Description |
|---|---|
| `multispace0` | Zero or more whitespace characters (default `ws`). |
| `multispace1` | One or more whitespace characters. |
| `space0` | Zero or more horizontal spaces. |
| `space1` | One or more horizontal spaces. |
| `line_ending` | `\n` or `\r\n`. |
| `empty` | Matches nothing (epsilon). |

### Return Types
Portable built-ins map to specific `winnow` return types:

| Portable Primitive | Return Type | Notes |
|---|---|---|
| `ident` | `String` | Consumes leading whitespace. |
| `string` | `String` | |
| `u32`, `i32`, `f64` | `u32`, `i32`, `f64` | |
| `bool` | `bool` | |
| `alpha`, `digit` | `char` | |

## Diagnostics

`winnow-grammar` provides compile-time checks to ensure your grammar is sound. It detects:
- **Indirect Left Recursion**: Cycles like `A -> B -> A`.
- **Unreachable Alternatives**: Shadowing detection.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

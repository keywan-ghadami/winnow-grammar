# winnow-grammar

[![Crates.io](https://img.shields.io/crates/v/winnow-grammar.svg)](https://crates.io/crates/winnow-grammar)
[![Documentation](https://docs.rs/winnow-grammar/badge.svg)](https://docs.rs/winnow-grammar)
[![License](https://img.shields.io/crates/l/winnow-grammar.svg)](https://github.com/keywan-ghadami/winnow-grammar/blob/main/LICENSE)

**winnow-grammar** is a powerful parser generator for Rust that allows you to define EBNF-like grammars directly inside your code. It compiles these definitions into efficient `winnow` parsers at compile time.

This crate is built on top of `syn-grammar-model` but targets the `winnow` parser combinator library. While `syn-grammar` is specialized for parsing Rust code (using `TokenStream`), `winnow-grammar` is designed for general-purpose parsing of text, data formats, and custom DSLs (using `&str` or `&[u8]`).

## Features

- **Inline Grammars**: Define your grammar directly in your Rust code using the `grammar!` macro.
- **EBNF Syntax**: Familiar syntax with sequences, alternatives (`|`), optionals (`?`), repetitions (`*`, `+`), and grouping `(...)`.
- **Type-Safe Actions**: Directly map parsing rules to Rust types and AST nodes using action blocks (`-> { ... }`).
- **Winnow Integration**: Generates efficient `winnow` parsers that work with standard `winnow` traits.
- **Automatic Left Recursion**: Write natural expression grammars (e.g., `expr = expr + term`) without worrying about infinite recursion.
- **Whitespace Handling**: Automatic whitespace skipping (configurable).
- **Rule Arguments**: Pass context or parameters between rules.
- **Span Tracking**: Support for `LocatingSlice` to track source positions (e.g., `rule @ span`).
- **Seamless Integration**: Easily mix generated rules with handwritten `winnow` parsers.

## Installation

Add `winnow-grammar` and `winnow` to your `Cargo.toml`.

```toml
[dependencies]
winnow-grammar = "0.1.0"
winnow = "0.6"
```

## Quick Start

Here is a complete example of a calculator grammar that parses mathematical expressions into an `i32`.

```rust
use winnow_grammar::grammar;
use winnow::prelude::*;
use winnow::stream::LocatingSlice;

grammar! {
    grammar Calc {
        // The return type of the rule is defined after `->`
        pub rule expression -> i32 =
            l:expression "+" r:term -> { l + r }
          | l:expression "-" r:term -> { l - r }
          | t:term                  -> { t }

        rule term -> i32 =
            f:factor "*" t:term -> { f * t }
          | f:factor "/" t:term -> { f / t }
          | f:factor            -> { f }

        rule factor -> i32 =
            i:integer           -> { i }
          | paren(e:expression) -> { e }
    }
}

fn main() {
    // The macro generates a module `Calc` containing a function `parse_expression`
    // corresponding to the `expression` rule.
    let input = "10 - 2 * 3";
    
    // We use LocatingSlice to support span tracking if needed, 
    // but a simple &str works too if the grammar doesn't use @ spans.
    let input = LocatingSlice::new(input);
    
    let result = Calc::parse_expression.parse(input);
    assert_eq!(result.unwrap(), 4);
}
```

### What happens under the hood?

The `grammar!` macro expands into a Rust module (named `Calc` in the example) containing:
- A function `parse_<rule_name>` for each rule (e.g., `parse_expression`).
- These functions take a `&mut I` where `I` is a `winnow` stream (e.g., `&str`, `LocatingSlice<&str>`).
- All necessary imports and helper functions to make the parser work.

## Detailed Syntax Guide

### Use Statements

You can include standard Rust `use` statements directly within your grammar block. These are passed through to the generated parser module, allowing you to easily import types needed for your rules.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar MyGrammar {
        use std::collections::HashMap;

        rule map -> HashMap<String, String> = 
            // ... implementation
            "test" -> { HashMap::new() }
    }
}
```

### Rules

A grammar consists of a set of rules. Each rule has a name, a return type, and a pattern to match.

```text
rule name -> ReturnType = pattern -> { action_code }
```

- **`name`**: The name of the rule (e.g., `expr`).
- **`ReturnType`**: The Rust type returned by the rule (e.g., `Expr`, `i32`, `Vec<String>`).
- **`pattern`**: The EBNF pattern defining what to parse.
- **`action_code`**: A Rust block that constructs the return value from the bound variables.

### Rule Arguments

Rules can accept arguments, allowing you to pass context or state down the parser chain.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar Args {
        rule main -> i32 = 
            "start" v:value(10) -> { v }

        rule value(offset: i32) -> i32 =
            i:integer -> { i + offset }
    }
}
```

### Patterns

#### Literals
Match specific strings. `winnow-grammar` automatically handles whitespace before literals.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar Kws {
        rule kw -> () = "fn" "name" -> { () }
    }
}
```

#### Built-in Parsers
`winnow-grammar` provides several built-in parsers for common text patterns. These are automatically generated with whitespace support.

| Parser | Description | Returns |
|--------|-------------|---------|
| `ident` | An alphanumeric identifier (including `_`) | `String` |
| `integer` | A decimal integer | `i32` |
| `uint` | A decimal unsigned integer | `u32` |
| `string` | A quoted string literal (supports escapes) | `String` |

#### Custom and External Rules
You can use any function that matches the `winnow` parser signature `Fn(&mut I) -> ModalResult<T>` as a rule. You just need to import it or define it in your crate.

```rust
use winnow::ascii::alpha1;
use winnow_grammar::grammar;

grammar! {
    grammar MyGrammar {
        use super::alpha1; // Import standard winnow parser

        rule word -> String = 
            w:alpha1 -> { w.to_string() }
    }
}
```

#### Sequences and Bindings
Match a sequence of patterns. Use `name:pattern` to bind the result to a variable available in the action block.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar Assignment {
        rule assignment -> (String, i32) = 
            name:ident "=" val:integer -> { 
                (name, val) 
            }
    }
}
```

#### Span Binding (`@`)
You can capture the `Span` (range) of a parsed rule using the syntax `name:rule @ span_var`. This requires your input type to implement `winnow::stream::Location` (e.g., `LocatingSlice`).

```rust
use winnow::stream::Range;
use winnow_grammar::grammar;

grammar! {
    grammar Spanned {
        rule main -> (String, Range) = 
            // Binds the identifier to `id` and its span to `s`
            id:ident @ s -> { (id, s) }
    }
}
```

#### Alternatives (`|`)
Match one of several alternatives. The first one that matches wins.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar Choice {
        rule choice -> bool = 
            "yes" -> { true }
          | "no"  -> { false }
    }
}
```

#### Repetitions (`*`, `+`, `?`)
- `pattern*`: Match zero or more times. Returns a `Vec`.
- `pattern+`: Match one or more times. Returns a `Vec`.
- `pattern?`: Match zero or one time. Returns an `Option`.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar List {
        rule list -> Vec<i32> = 
            "[" elements:integer* "]" -> { elements }
    }
}
```

#### Delimiters
Match content inside delimiters. These handle whitespace automatically around the delimiters.

- `paren(pattern)`: Matches `( pattern )`.
- `[ pattern ]`: Matches `[ pattern ]`.
- `{ pattern }`: Matches `{ pattern }`.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar Tuple {
        rule tuple -> (i32, i32) = 
            paren(a:integer "," b:integer) -> { (a, b) }
    }
}
```

## Advanced Topics

### Left Recursion

Recursive descent parsers typically struggle with left recursion (e.g., `A -> A b`). `winnow-grammar` automatically detects direct left recursion and compiles it into an iterative loop. This makes writing expression parsers natural and straightforward.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar Expr {
        // This works perfectly!
        rule expr -> i32 = 
            l:expr "+" r:term -> { l + r }
          | t:term            -> { t }
          
        rule term -> i32 = i:integer -> { i }
    }
}
```

### Whitespace Handling

By default, `winnow-grammar` assumes you want to skip whitespace between tokens. It inserts a parser equivalent to `winnow::ascii::multispace0` before every literal, built-in, and delimiter.

If you need precise control over whitespace (e.g., for whitespace-sensitive languages), you may need to implement custom rules or override the default behavior (future versions will provide more configuration options for this).

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

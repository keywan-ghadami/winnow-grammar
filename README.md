# winnow-grammar

**winnow-grammar** is a powerful parser generator for Rust that allows you to define EBNF-like grammars directly inside your code. It compiles these definitions into efficient `winnow` parsers at compile time.

This crate is built on top of `syn-grammar-model` but targets the `winnow` parser combinator library instead of `syn`. This makes it ideal for parsing text formats, configuration files, and custom DSLs at runtime.

## Features

- **Inline Grammars**: Define your grammar directly in your Rust code using the `grammar!` macro.
- **EBNF Syntax**: Familiar syntax with sequences, alternatives (`|`), optionals (`?`), repetitions (`*`, `+`), and grouping `(...)`.
- **Type-Safe Actions**: Directly map parsing rules to Rust types using action blocks (`-> { ... }`).
- **Winnow Integration**: Generates efficient `winnow` parsers (`PResult<T>`).
- **Automatic Left Recursion**: Write natural expression grammars (e.g., `expr = expr + term`) without worrying about infinite recursion.
- **Rule Arguments**: Pass context or parameters between rules.
- **Span Tracking**: Support for `LocatingSlice` to track source positions (e.g., `rule @ span`).

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
    let input = LocatingSlice::new(input);
    let result = Calc::parse_expression.parse(input);
    assert_eq!(result.unwrap(), 4);
}
```

### What happens under the hood?

The `grammar!` macro expands into a Rust module (named `Calc` in the example) containing:
- A function `parse_<rule_name>` for each rule (e.g., `parse_expression`).
- These functions are generic over the input type `I`, requiring it to implement `Stream`, `Location`, and text-related traits.
- Typically, you pass `LocatingSlice<&str>` (or similar) to these functions.

## Detailed Syntax Guide

### Rules

A grammar consists of a set of rules. Each rule has a name, a return type, and a pattern to match.

```rust,ignore
rule name -> ReturnType = pattern -> { action_code }
```

- **`name`**: The name of the rule (e.g., `expr`).
- **`ReturnType`**: The Rust type returned by the rule (e.g., `Expr`, `i32`, `Vec<String>`).
- **`pattern`**: The EBNF pattern defining what to parse.
- **`action_code`**: A Rust block that constructs the return value from the bound variables.

### Patterns

#### Literals
Match specific strings.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar example {
        rule kw -> () = "fn" -> { () }
        //   ^     ^    ^       ^
        //  Name  Type Pattern Action (Return Value)
    }
}
```

##### Explanation
* **rule kw**: The name of the defined rule is `kw`.
* **-> ()**: The rule returns `()` (Unit) as its result type.
* **= "fn"**: The rule matches only if the input corresponds exactly to the string "fn".
* **-> { () }**: This is the action code block executed upon a successful match.


#### Built-in Parsers
`winnow-grammar` provides built-in parsers for common patterns:

| Parser | Description | Returns |
|--------|-------------|---------|
| `ident` | An identifier (alphanumeric + `_`) | `String` |
| `integer` | An integer literal | `i32` (or parsed type) |
| `string` | A quoted string literal | `String` |

*(Note: The exact set of built-ins is evolving)*

#### Sequences and Bindings
Match a sequence of patterns. Use `name:pattern` to bind the result to a variable available in the action block.

```rust
use winnow_grammar::grammar;

#[derive(Debug, PartialEq)]
pub enum Stmt {
    Assign(String, i32),
}

grammar! {
    grammar Assignment {
        rule assignment -> Stmt = 
            name:ident "=" val:integer -> { 
                Stmt::Assign(name, val) 
            }
    }
}

# // Required to keep `Stmt` in module scope. Without this, rustdoc wraps the code in a function, making `Stmt` invisible to the macro-generated module.
# fn main() {}
```

#### Alternatives (`|`)
Match one of several alternatives. The first one that matches wins.

```rust
use winnow_grammar::grammar;

grammar! {
    grammar Boolean {
        rule boolean -> bool = 
            "true"  -> { true }
          | "false" -> { false }
    }
}
```

#### Repetitions (`*`, `+`, `?`)
- `pattern*`: Match zero or more times. Returns a `Vec`.
- `pattern+`: Match one or more times. Returns a `Vec`.
- `pattern?`: Match zero or one time. Returns an `Option` (or `()` if unbound).

```rust
use winnow_grammar::grammar;

grammar! {
    grammar List {
        // Matches "[ 1 2 3 ]"
        rule list -> Vec<i32> = 
            "[" elements:integer* "]" -> { elements }
    }
}
```

#### Delimiters
Match content inside delimiters.

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

#### Spans (`@`)
Capture the source span (range) of a parsed element.

```rust,ignore
rule spanned_term -> (Expr, std::ops::Range<usize>) =
    t:term @ s -> { (t, s) }
```

### Whitespace Handling

`winnow-grammar` automatically handles whitespace for you. It inserts a `multispace0` parser before every literal and built-in token (like `ident`, `integer`, etc.). This means you don't need to manually handle spaces, tabs, or newlines in your grammar.

Example:
```rust
use winnow_grammar::grammar;

grammar! {
    grammar List {
        rule list -> Vec<i32> = "[" i:integer* "]" -> { i }
    }
}
```

This will successfully parse `[ 1 2 3 ]`, `[1 2 3]`, or even:
```text
[
  1
  2
  3
]
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

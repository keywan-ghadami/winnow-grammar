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
    let mut input = "10 - 2 * 3";
    let result = Calc::parse_expression.parse(&mut input);
    assert_eq!(result.unwrap(), 4);
}
```

### What happens under the hood?

The `grammar!` macro expands into a Rust module (named `Calc` in the example) containing:
- A function `parse_<rule_name>` for each rule (e.g., `parse_expression`).
- These functions take `&mut &str` (input) and return `winnow::PResult<T>`.
- All necessary imports and helper functions to make the parser work.

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

```rust,ignore
rule assignment -> Stmt = 
    name:ident "=" val:expr -> { 
        Stmt::Assign(name, val) 
    }
```

#### Alternatives (`|`)
Match one of several alternatives. The first one that matches wins.

```rust,ignore
rule boolean -> bool = 
    "true"  -> { true }
  | "false" -> { false }
```

#### Repetitions (`*`, `+`, `?`)
- `pattern*`: Match zero or more times. Returns a `Vec`.
- `pattern+`: Match one or more times. Returns a `Vec`.
- `pattern?`: Match zero or one time. Returns an `Option` (or `()` if unbound).

```rust,ignore
rule list -> Vec<i32> = 
    [ elements:integer* ] -> { elements }
```

#### Delimiters
Match content inside delimiters.

- `paren(pattern)`: Matches `( pattern )`.
- `[ pattern ]`: Matches `[ pattern ]`.
- `{ pattern }`: Matches `{ pattern }`.

```rust,ignore
rule tuple -> (i32, i32) = 
    paren(a:integer "," b:integer) -> { (a, b) }
```

### Whitespace Handling

`winnow-grammar` automatically handles whitespace for you. It inserts a `multispace0` parser before every literal and built-in token (like `ident`, `integer`, etc.). This means you don't need to manually handle spaces, tabs, or newlines in your grammar.

Example:
```rust,ignore
rule list -> Vec<i32> = "[" i:integer* "]" -> { i }
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

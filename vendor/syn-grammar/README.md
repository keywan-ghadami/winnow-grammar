# syn-grammar

[![Crates.io](https://img.shields.io/crates/v/syn-grammar.svg)](https://crates.io/crates/syn-grammar)
[![Documentation](https://docs.rs/syn-grammar/badge.svg)](https://docs.rs/syn-grammar)
[![License](https://img.shields.io/crates/l/syn-grammar.svg)](https://github.com/keywan-ghadami/syn-grammar/blob/main/LICENSE)

**syn-grammar** is a powerful parser generator for Rust that allows you to define EBNF-like grammars directly inside your code. It compiles these definitions into efficient `syn` parsers at compile time.

Writing parsers for procedural macros or Domain Specific Languages (DSLs) in Rust often involves writing repetitive boilerplate code using the `syn` crate. **syn-grammar** simplifies this process by letting you describe *what* you want to parse using a clean, readable syntax, while handling the complex logic of parsing, backtracking, and error reporting for you.

## Features

- **Inline Grammars**: Define your grammar directly in your Rust code using the `grammar!` macro.
- **EBNF Syntax**: Familiar syntax with sequences, alternatives (`|`), optionals (`?`), repetitions (`*`, `+`), and grouping `(...)`.
- **Type-Safe Actions**: Directly map parsing rules to Rust types and AST nodes using action blocks (`-> { ... }`).
- **Seamless Syn Integration**: First-class support for parsing Rust tokens like identifiers, literals, types, and blocks.
- **Portable Primitives**: A core set of built-ins (`ident`, `u32`, `i64`, `alpha`) are conceptually portable, allowing other backends like `winnow-grammar` to provide their own efficient implementations.
- **Automatic Left Recursion**: Write natural expression grammars (e.g., `expr = expr + term`) without worrying about infinite recursion.
- **Backtracking & Ambiguity**: Automatically handles ambiguous grammars with speculative parsing.
- **Cut Operator**: Control backtracking explicitly for better error messages and performance.
- **Lookahead**: Use `peek(...)` and `not(...)` for positive and negative lookahead assertions.
- **Rule Arguments**: Pass context or parameters between rules.
- **Grammar Inheritance**: Reuse rules from other grammars.
- **Testing Utilities**: Fluent API for testing your parsers with pretty-printed error reporting.

## Installation

### 1. Quick Installation (Runtime Parsing)

Use this setup if you want to parse strings **at runtime** inside your application. This is the standard approach for:
- **CLIs & Interpreters**: Parsing user input or commands.
- **Configuration Files**: Reading custom config formats at startup.
- **Prototyping**: Experimenting with grammars in `main.rs`.

Add `syn-grammar` and `syn` to your `Cargo.toml`. `syn` is required at runtime because the generated parser relies on its types (e.g., `ParseStream`, `Ident`).

```toml
[dependencies]
syn-grammar = "0.7.0"
syn = { version = "2.0", features = ["full", "extra-traits"] }
quote = "1.0"
proc-macro2 = "1.0"
```

### 2. Optimized Installation (Compile-Time Macros)

If you are writing a **procedural macro** to parse input **at compile time**, you should isolate your parser definition in a separate crate. This is the correct approach for:

- **Embedded DSLs**: Parsing custom syntax inside Rust code (e.g., HTML-like templates, State Machines, SQL-like queries).
- **Code Generation**: Reading an external definition file during the build and generating Rust code from it.
- **Compile-Time Verification**: Checking syntax or configuration validity during `cargo build`.

**Steps:**

1. Create a separate `proc-macro` crate.
2. Add `syn-grammar`, `syn`, and `quote` to **that** crate\'s `Cargo.toml`.
3. Define your grammar and macro there.
4. Depend on that crate from your main project.

**Why?** Your main project will use the macro to generate code, but the heavy `syn` parsing logic will not be compiled into your final binary. This significantly improves build times for users of your macro.

### ⚠️ Important Note on Tokenization

Since `syn-grammar` is built on top of `syn`, it uses the **Rust Tokenizer**. This means your grammar must consist of valid Rust tokens.

- **Good Use Cases**: Grammars that look somewhat like code or data structures (e.g., JSON, mathematical expressions, C-like syntax, HTML tags).
- **Limitations**: You cannot parse languages that require a custom lexer, such as:
    - **Whitespace-sensitive languages** (e.g., Python, YAML) — `syn` skips whitespace automatically.
    - **Binary formats**.
    - **Arbitrary text** that doesn\'t form valid Rust tokens (e.g., unquoted strings with special characters like `@` or `$` in positions Rust doesn\'t allow).

## Quick Start

Here is a complete example of a calculator grammar that parses mathematical expressions into an `i32`.

```rust
use syn_grammar::grammar;
use syn::parse::Parser; // Required for .parse_str()

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
            i:i32               -> { i }
          | paren(e:expression) -> { e }
    }
}

fn main() {
    // The macro generates a module `Calc` containing a function `parse_expression`
    // corresponding to the `expression` rule.
    let result = Calc::parse_expression.parse_str("10 - 2 * 3");
    assert_eq!(result.unwrap(), 4);
}
```

### What happens under the hood?

The `grammar!` macro expands into a Rust module (named `Calc` in the example) containing:
- A function `parse_<rule_name>` for each rule (e.g., `parse_expression`).
- These functions take a `syn::parse::ParseStream` and return a `syn::Result<T>`.
- All necessary imports and helper functions to make the parser work, including `use super::*;` for convenience.

## Detailed Syntax Guide

### Use Statements

You can include standard Rust `use` statements directly within your grammar block. These are passed through to the generated parser module, allowing you to easily import types needed for your rules.

```rust
use syn_grammar::grammar;

grammar! {
    grammar MyGrammar {
        use std::collections::HashMap;
        use syn::Ident;

        rule map -> HashMap<String, String> = 
            // ... implementation using HashMap
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

#### Attributes and Doc Comments

Rules can be decorated with standard Rust attributes and documentation comments. These are passed through to the generated function.

```rust
use syn_grammar::grammar;
use syn::Ident;
use syn_grammar::Identifier;

grammar! {
    grammar MyGrammar {
        /// Parses a valid identifier.
        #[cfg(feature = "extra")]
        rule my_ident -> Identifier = i:ident -> { i }
    }
}

```

### Rule Arguments

Rules can accept arguments, allowing you to pass context or state down the parser chain.

```rust
use syn_grammar::grammar;

grammar! {
    grammar Args {
        rule main -> i32 = 
            "start" v:value(10) -> { v }

        rule value(offset: i32) -> i32 =
            i:i32 -> { i + offset }
    }
}
```

### Grammar Inheritance

You can inherit rules from another grammar module. This is useful for splitting large grammars or reusing common rules.

```rust
use syn_grammar::grammar;

grammar! {
    grammar Base {
        pub rule num -> i32 = i:i32 -> { i }
    }
}

grammar! {
    grammar Derived : Base {
        rule main -> i32 = 
            "add" a:num b:num -> { a + b }
    }
}
# fn main() {}
```

### Patterns

#### Literals and Keywords
Match specific tokens using string literals.

```rust
use syn_grammar::grammar;

grammar! {
    grammar Kws {
        rule kw -> () = "fn" "name" -> { () }
    }
}
```

#### Multi-token Literals
You can match sequences of tokens that must appear strictly adjacent to each other (no whitespace) by using a single string literal containing multiple tokens.

```rust
use syn_grammar::grammar;

grammar! {
    grammar Tokens {
        // Matches "?." (e.g. in `foo?.bar`)
        // Fails if there is a space like `? .`
        rule optional_dot -> () = "?." -> { () }

        // Matches "@detached" (Punct `@` + Ident `detached`) without space
        rule attribute -> () = "@detached" -> { () }
    }
}
```

#### Built-in Parsers
`syn-grammar` provides a rich set of built-in parsers. They are divided into two categories:

**1. Portable Built-ins**

These represent high-level, conceptually portable primitives that other backends (like `winnow-grammar`) are expected to implement. A grammar using only these should be portable.

**Core Primitives**

| Parser | Description | Returns |
|---|---|---|
| `ident` | A Rust identifier | `syn_grammar::Identifier` |
| `string` | A string literal's content | `syn_grammar::StringLiteral` |
| `alpha` | An alphabetic identifier | `syn::Ident` |
| `digit` | A numeric identifier | `syn::Ident` |
| `whitespace` | Ensures token separation | `()` |
| `outer_attrs` | Parses `#[...]` attributes | `Vec<syn::Attribute>` |

**Numeric Types (Consistent Naming)**

We implement a comprehensive naming convention for numeric types.

| Category | Grammar Name | Return Type (Rust) | Aliases |
|---|---|---|---|
| **Signed** | `i8`, `i16`, `i32`, `i64`, `i128`, `isize` | `i8`, `i16`, `i32`, `i64`, `i128`, `isize` | |
| **Unsigned** | `u8`, `u16`, `u32`, `u64`, `u128`, `usize` | `u8`, `u16`, `u32`, `u64`, `u128`, `usize` | |
| **Float** | `f32`, `f64` | `f32`, `f64` | |
| **Alt Bases** | `hex_literal`, `oct_literal`, `bin_literal` | `u64` | |

*Note: For alternative bases (`hex`, `oct`, `bin`), parsing is done into a maximum-width unsigned container (`u64`) to avoid combinatorial type explosion. Use developer action blocks for explicit downcasting.*

**2. `syn`-Specific Built-ins**

These are tied to the `syn` crate's AST and are not portable.

| Parser | Description | Returns |
|---|---|---|
| `rust_type` | A Rust type (e.g., `Vec<i32>`) | `syn::Type` |
| `rust_block` | A block of code (e.g., `{ stmt; }`) | `syn::Block` |
| `lit_str` | A string literal object | `syn::LitStr` |
| `lit_int` | A typed integer literal (e.g. `1u8`) | `syn::LitInt` |

### Overriding Built-ins & Custom Rules

If you need to change how a built-in works or define a reusable rule that isn't part of the standard set, you have two options:

#### 1. Local Override
You can shadow a built-in rule by defining a rule with the same name in your grammar block.

```rust,ignore
use syn_grammar::grammar;
use syn::Token;

grammar! {
    grammar MyGrammar {
        // Overrides the default 'ident' behavior
        rule ident -> String =
            i:ident -> { i.to_string().to_uppercase() }
    }
}
```

#### 2. Import Injection
You can import a function that matches the expected signature (`fn(ParseStream) -> Result<T>`) and use it as a terminal rule.

```rust,ignore
use syn_grammar::grammar;

// In some other module
pub struct MyType;
pub fn my_custom_parser(input: syn::parse::ParseStream) -> syn::Result<MyType> {
    // ... custom parsing logic
    Ok(MyType)
}

grammar! {
    grammar MyGrammar {
        use super::my_custom_parser; // Import it

        rule main -> MyType = 
            // Use it like any other rule
            val:my_custom_parser -> { val }
    }
}
```
This is particularly useful for library authors who want to provide a "prelude" of custom parsers for their users.

#### Sequences and Bindings
Match a sequence of patterns. Use `name:pattern` to bind the result to a variable available in the action block. As of v0.6.0, generated parsers automatically include `use super::*;`, allowing you to refer to items from the parent module (like `Stmt` in the example below) without a `super::` prefix.

```rust
use syn_grammar::grammar;
use syn::Ident;
use syn_grammar::Identifier;

// Mock Stmt for the example
pub enum Stmt {
    Assign(Identifier, i32),
}

grammar! {
    grammar Assignment {
        rule assignment -> Stmt = 
            name:ident "=" val:expr -> { 
                Stmt::Assign(name, val) 
            }
            
        rule expr -> i32 = i:i32 -> { i }
    }
}
# fn main() {}
```

#### Span Binding (`@`)
You can capture the `Span` of a parsed rule or built-in using the syntax `name:rule @ span_var`. This is useful for error reporting or constructing spanned AST nodes.

**Note**: The rule being bound must return a type that implements `syn::spanned::Spanned` (e.g., `syn::Ident`, `syn::Type`, `syn::LitStr`, and `syn_grammar::Identifier`). Primitive types like `i32` or `String` do not support this.

```rust
use syn_grammar::grammar;
use syn_grammar::Identifier;

grammar! {
    grammar Spanned {
        rule main -> (Identifier, proc_macro2::Span) = 
            // Binds the identifier to `id` and its span to `s`
            id:ident @ s -> { (id, s) }
    }
}
# fn main() {}
```

#### Alternatives (`|`)
Match one of several alternatives. The first one that matches wins.

```rust
use syn_grammar::grammar;

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
- `pattern?`: Match zero or one time. Returns an `Option` (or `()` if unbound).

```rust
use syn_grammar::grammar;

grammar! {
    grammar List {
        rule list -> Vec<i32> = 
            [ elements:i32* ] -> { elements }
    }
}
```

#### Groups `(...)`
Group patterns together to apply repetitions or ensure precedence.

```rust
use syn_grammar::grammar;

grammar! {
    grammar Group {
        rule complex -> () = 
            ("a" | "b")+ "c" -> { () }
    }
}
```

#### Delimiters
Match content inside delimiters.

**Note**: You cannot match delimiters using string literals (e.g., `\"[\"` or `\"}\"`) because `syn` parses them as structural `TokenTree`s. You must use the syntax below.

- `paren(pattern)`: Matches `( pattern )`.
- `[ pattern ]`: Matches `[ pattern ]`.
- `{ pattern }`: Matches `{ pattern }`.

```rust
use syn_grammar::grammar;

grammar! {
    grammar Tuple {
        rule tuple -> (i32, i32) = 
            paren(a:i32 "," b:i32) -> { (a, b) }
    }
}
```

#### Lookahead (`peek`, `not`)
Lookahead operators allow you to check for a pattern without consuming input.

- `peek(pattern)`: Succeeds if `pattern` matches. Input is not advanced.
- `not(pattern)`: Succeeds if `pattern` does *not* match. Input is not advanced.

```rust
use syn_grammar::grammar;

grammar! {
    grammar Lookahead {
        // Matches "a" only if followed by "b", but "b" is not consumed
        rule check -> () = "a" peek("b") -> { () }
        
        // Matches "a" only if NOT followed by "c"
        rule neg -> () = "a" not("c") -> { () }
    }
}
```

#### Error Recovery (`recover`)
You can make your parser robust against errors using `recover(rule, sync_token)`.
If `rule` fails, the parser will skip tokens until it finds `sync_token`, returning `None` (or `(None, ...)` for bindings).
Note that `recover` does **not** consume the sync token.

```rust
use syn_grammar::grammar;

#[derive(Debug)]
pub struct Stmt;

grammar! {
    grammar Recovery {
        rule stmt -> Option<Stmt> =
            // If `parse_stmt` fails, skip until `;`
            // `s` will be `Option<Stmt>` (Some if success, None if recovered)
            s:recover(parse_stmt, ";") ";" -> { s }
            
        rule parse_stmt -> Stmt = "let" "x" -> { Stmt }
    }
}
# fn main() {}
```

### The Cut Operator (`=>`)

The cut operator `=>` allows you to commit to a specific alternative. If the pattern *before* the `=>` matches, the parser will **not** backtrack to try other alternatives, even if the pattern *after* the `=>` fails. This produces better error messages.

```rust
use syn_grammar::grammar;
use syn::Ident;
use syn_grammar::Identifier;

pub enum Stmt {
    Let(Identifier, i32),
    Expr(i32),
}

grammar! {
    grammar Cut {
        rule stmt -> Stmt =
            // If we see "let", we commit to this rule. 
            // If "mut" or the identifier is missing, we error immediately 
            // instead of trying the next alternative.
            "let" => "mut"? name:ident "=" e:expr -> { Stmt::Let(name, e) }
          | e:expr -> { Stmt::Expr(e) }
          
        rule expr -> i32 = i:i32 -> { i }
    }
}
# fn main() {}
```

## Testing

`syn-grammar` provides a fluent testing API via the `grammar-kit` crate (re-exported as `syn_grammar::testing`). When tests fail, errors are pretty-printed with source context and underlining.

```rust,no_run
use syn_grammar::grammar;
use syn_grammar::testing::Testable;

grammar! {
    grammar Calc {
        rule expression -> i32 = 
            l:expression "+" r:term -> { l + r }
          | t:term -> { t }
        
        rule term -> i32 = i:i32 -> { i }
    }
}

#[test]
fn test_calc() {
    Calc::parse_expression
        .parse_str("1 + 2")
        .test()
        .assert_success_is(3);

    Calc::parse_expression
        .parse_str("1 + *")
        .test()
        .assert_failure_contains("expected term");
        
}
# fn main() {}
```

## Advanced Topics

### Left Recursion

Recursive descent parsers typically struggle with left recursion (e.g., `A -> A b`). `syn-grammar` automatically detects direct left recursion and compiles it into an iterative loop. This makes writing expression parsers natural and straightforward.

```rust
use syn_grammar::grammar;

grammar! {
    grammar Expr {
        // This works perfectly!
        rule expr -> i32 = 
            l:expr "+" r:term -> { l + r }
          | t:term            -> { t }
          
        rule term -> i32 = i:i32 -> { i }
    }
}
```

### Backtracking

By default, `syn-grammar` uses `syn`\'s speculative parsing (`fork`) to try alternatives.
1. It checks if the next token matches the start of an alternative (using `peek`).
2. If ambiguous, it attempts to parse the alternative.
3. If it fails, it backtracks and tries the next one.

This allows for flexible grammars but can impact performance if overused. Use the **Cut Operator** (`=>`) to prune the search space when possible.

## Building Custom Backends

If you are a library author who wants to create a parser generator using `syn-grammar`'s syntax (e.g. `winnow-grammar` or `chumsky-grammar`), you can use `syn-grammar-model` as a reusable frontend.

See [EXTENDING.md](EXTENDING.md) for a guide on how to build custom backends.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

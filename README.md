# winnow-grammar

[![Crates.io](https://img.shields.io/crates/v/winnow-grammar.svg)](https://crates.io/crates/winnow-grammar)
[![Documentation](https://docs.rs/winnow-grammar/badge.svg)](https://docs.rs/winnow-grammar)
[![License](https://img.shields.io/crates/l/winnow-grammar.svg)](https://github.com/keywan-ghadami/winnow-grammar/blob/main/LICENSE)

**Write the grammar. Get the parser, the error messages, the streaming and the parallelism for free.**

winnow-grammar turns an EBNF-like grammar, written inline in your Rust code,
into fast [`winnow`](https://crates.io/crates/winnow) parsers at compile time.
You describe *what* the language looks like; the boilerplate a hand-written
parser makes you repeat in every function is generated: whitespace
between tokens, positions, backtracking, error positions and the rule stack
behind them.

```rust,ignore
grammar! {
    grammar Calc {
        pub expr -> Expr =
            l:expr "+" r:term -> { Expr::Add(Box::new(l), Box::new(r)) }
          | t:term -> { t }

        term -> Expr =
            l:term "*" r:atom -> { Expr::Mul(Box::new(l), Box::new(r)) }
          | a:atom -> { a }

        atom -> Expr =
            n:i64          -> { Expr::Num(n) }
          | name:ident     -> { Expr::Var(name) }
          | paren(e:expr)  -> { e }
    }
}

let ast = Calc::parse_expr().parse_next(&mut input)?;   // "2 * (x + 1)"
```

That is the whole parser: precedence, parentheses, interned identifiers and
whitespace included.

## Why winnow-grammar

**The grammar is the documentation.** A rule reads like the specification of
the language, not like a parser that implements it. Left recursion is
written the way the language is defined (`expr = expr "+" term`) and turned
into a loop for you - no rewriting into `separated` folds by hand. (A
left-recursive rule's type must be `Clone`.)

**Good error messages without writing any.** A failed parse says what was
expected, what was found, where - with the line and a caret - and which rules
it was inside:

```text
expected one of: `&`, identifier; found unexpected token `)` at line 1, column 9
in ty
in arg
```

A parse that succeeds pays nothing for this: the diagnosis is made only when
it fails. `recover(rule, sync)` keeps going after an error and collects every
one of them, as an editor or a compiler wants.

**Mistakes in the grammar are compile errors, at the line that has them** -
not a stack overflow at runtime, not a branch that silently never matches:

```text
error: rule `comment` is used by `WS` but is syntactic (lowercase): a syntactic rule
       skips whitespace at its start by calling `WS`, so `WS -> comment -> WS` recurses
       without consuming input. Rules used for whitespace must be lexical - name it `COMMENT`.
  --> src/main.rs:10:9
   |
10 |         comment = "//" until(line_ending)
   |         ^^^^^^^
```

The same goes for indirect left recursion (`a -> b -> a`), identical
alternatives that make one of them dead code, and misused declarations: each
says what is wrong and what to write instead.

**One grammar, every way of reading.** The grammar you wrote for a `&str` in
memory also reads bytes (`input bytes;`), a stream larger than memory
(`#[stream]`, from any `std::io::Read`, in bounded memory) and runs on every
core (`par_fold`) - with the same results and the same error messages. You
do not rewrite your parser when the input grows.

**Still winnow underneath.** Every rule becomes a plain `impl Parser`. A
hand-written winnow parser joins a grammar as an `extern rule`, and generated
rules work inside your own combinators; you never give up control for convenience. And
the generated code is built for speed: ASCII classes are scanned a word at a
time, UTF-8 is decoded only where text is actually made, and identifiers are
interned into 4-byte symbols.

**Testing is a one-liner.** `winnow_grammar::testing` gives every rule
`.parse_test("…")` with assertions on the value or on the error message.

## Documentation

- [SYNTAX.md](SYNTAX.md) - the grammar language, the generated entry points,
  frames and parallel parsing, reading from a stream, error messages.
- [docs/adr/](docs/adr/) - why things are as they are, and what was measured.
- [CHANGELOG.md](CHANGELOG.md) - what changed.

## Features

- **Inline Grammars**: Define your grammar directly in your Rust code using the `grammar!` macro.
- **Type-Safe Actions**: Directly map parsing rules to Rust types and AST nodes using action blocks (`-> { ... }`).
- **Winnow Integration**: Generates efficient `winnow` parsers that work with standard `winnow` traits.
- **Whitespace Handling**: Automatic whitespace skipping (configurable).
- **Span Tracking**: Support for `LocatingSlice` to track source positions (e.g., `rule @ span`).
- **Parallel parsing**: `par_fold` over a `#[frame]` cuts a large input into pieces parsed on every core.
- **Streaming input**: parse from any `std::io::Read` - a pipe, a socket, a file larger than memory - in bounded memory, with the answer of the parse in memory.

## Installation

Add `winnow-grammar` and `winnow` to your `Cargo.toml`.

```toml
[dependencies]
winnow-grammar = "0.2.0"
winnow = "1"
```

## Quick Start

Here is a complete example of a Cron expression parser.

```rust
use winnow_grammar::{grammar, ParseContext};
use winnow::prelude::*;
use winnow::stream::{LocatingSlice, Stateful};

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
    // Generated parsers run on a `Stateful` stream: `LocatingSlice` provides the
    // spans, `ParseContext` carries the shared state (string interner, user
    // state). `winnow_grammar::ParseInput<'_>` is an alias for exactly this.
    let mut stream = Stateful {
        state: ParseContext::<()>::default(),
        input: LocatingSlice::new("0 30 9 * * 1-5"),
    };

    // `parse_schedule()` builds the parser; `.parse_next(&mut stream)` runs it.
    let result = Cron::parse_schedule().parse_next(&mut stream);
    println!("{:?}", result);
}
```

### Error messages

Failures come back as `winnow_grammar::ParseError` with the expectation, what
was found, the position and the rule stack — see *Error Messages* in
[SYNTAX.md](SYNTAX.md). The diagnosis is only made when a parse fails, by
parsing again with the full engine (`ParseContext::diagnose` chooses; ADR 17).

### What happens under the hood?

The `grammar!` macro expands into a Rust module containing:
- A function `parse_<rule_name>()` for each rule. It is a parser **factory**:
  calling it returns an `impl Parser`, which you then drive with `.parse(input)`
  or `.parse_next(&mut input)`.
- The returned parser takes a `&mut I` where `I` is a `winnow` stream (e.g.,
  `&str`, `LocatingSlice<&str>`).

## Backend Specifics

### Input Type
The generated parsers take `&str`: the input type is
`winnow_grammar::ParseInput<'a, S>`, a `winnow` `LocatingSlice<&'a str>` carrying
the shared `ParseContext`. A grammar that declares `input bytes;` takes `&[u8]`
instead - `winnow_grammar::ParseBytes<'a, S>`, the same context over a
`LocatingSlice<&'a [u8]>` - and checks as UTF-8 only what it turns into text:
`text(p)`, `intern(p)`, `raw_ident`, `string`, `char`. Literals and ASCII
classes compare bytes and are not checked; `bytes(p)` and `until(t)` yield the
bytes as they are. A byte that is not UTF-8 where text is made is a cut at its
own offset, `invalid UTF-8 at byte N: 0xXX`. See *Byte input* in the syntax
guide and ADR 25.

If you use **Span Binding (`@`)**, your input type **must** implement `winnow::stream::Location`. The recommended type for this is `winnow::stream::LocatingSlice`.

### Whitespace Handling
By default, `winnow-grammar` automatically skips whitespace between tokens in syntactic rules (rules with lowercase names). The default whitespace parser is equivalent to `winnow::ascii::multispace0`.

You can override this behavior by defining a special rule named `ws`. This is a powerful feature for handling more complex spacing, like comments. For example, to make your parser treat `//` style comments as whitespace:

```rust
use winnow_grammar::{grammar, ParseContext};
use winnow::prelude::*;
use winnow::stream::{LocatingSlice, Stateful};

grammar! {
    grammar CommentAware {
        // Override 'ws' to skip spaces, newlines, and single-line comments.
        WSE = multispace1
        WS = (WSE | COMMENT)*

        // A rule that recognizes a single-line comment.
        // `line_ending` is a built-in parser. `until` consumes input up to it.
        //
        // The name is UPPERCASE on purpose: that makes it a *lexical* rule. A
        // lowercase `comment` would be syntactic, so the generator would insert
        // `WS` between its own tokens - and `WS` calls the comment rule. That
        // cycle recurses until the stack is gone.
        COMMENT = "//" until(line_ending)

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
    let mut stream = Stateful {
        state: ParseContext::<()>::default(),
        input: LocatingSlice::new("10 // add 20\n + 20"),
    };
    let result = CommentAware::parse_add().parse_next(&mut stream).unwrap();
    assert_eq!(result, 30);
}
```

### Built-ins
In addition to the portable built-ins (see [SYNTAX.md](SYNTAX.md)), `winnow-grammar` provides the following `winnow`-specific parsers:

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
| `ident` | `Symbol` | Interned in the context's interner - `intern(raw_ident)`. |
| `raw_ident` | `&'a str` | The same characters, not interned. |
| `intern(p)` | `Symbol` | Interns whatever `p` yields. |
| `string` | `&'a str` | Borrowed from the input, quotes excluded. |
| `alpha1`, `digit1`, `hex_digit1` | `&'a str` | |
| `u32`, `i32`, `f64` | `u32`, `i32`, `f64` | |
| `bool` | `bool` | |
| `char`, `any`, `digit`, `hex_digit` | `char` | |

A `Symbol` is a 4-byte id; `ctx.interner.resolve(sym)` gives the text back, and
`sym.index()` is its dense position in the interner (`0..interner.len()`) - the
row number for a caller's own `Vec` of accumulated data. Symbols and indices
are only meaningful against the interner that made them - see the `par_fold`
note in [SYNTAX.md](SYNTAX.md).

## Reading from a stream

A grammar's input does not have to be in memory. Mark the rule whose `fold`
holds the many items, and read from anything that implements `Read`:

```rust,ignore
grammar! {
    grammar Rows {
        #[stream]
        pub file -> u64 = s:fold(row, || 0u64, |a: u64, v: u64| a + v) -> { s }
        row -> u64 = name:raw_ident ";" v:u64 -> { v }
    }
}

let mut context = ParseContext::<()>::default();
let sum = Rows::parse_file_reader(std::io::stdin(), &mut context, Reading::default())
    .map_err(|e| e.render_excerpt())?;
```

Memory stays at about `Reading::max_frame` (16 MiB by default) however long
the input is, and the result - or the error, at its offset in the stream - is
the one the parse in memory gives. Records a byte search can find (lines)
can also be read by a `par_fold` rule, in parallel. See *Reading from a
stream* in [SYNTAX.md](SYNTAX.md) for which to use, what a `#[stream]` rule may
look like, what it costs, and what a hand-written parser must do in such a
grammar.

## Diagnostics

`winnow-grammar` provides compile-time checks to ensure your grammar is sound. It detects:
- **Indirect left recursion**: cycles like `A -> B -> A`. (Direct left recursion,
  `expr = expr "+" term`, is supported.)
- **Unreachable alternatives**: an alternative that an earlier one shadows.
- **Whitespace cycles**: a rule used by `WS` that would itself skip whitespace.
- **Misused declarations**: `state`, `interner`, `input`, `#[stream]`,
  `#[frame]` and `fold` arguments in places they cannot work.

Each points at the offending line of the grammar and says what to write
instead (`tests/ui/*.stderr` has every message).

## Tuning

### Cargo features

`ahash` is on by default; the others are off. The crate is complete without
any of them.

| feature | what it does |
|---|---|
| `rayon` | `parse_<rule>_pieces` runs the pieces of a `par_fold` rule on rayon's global pool. Without it the same driver runs them in sequence - the same cut, the same answer - which is what lets a test check the split without threads. |
| `ahash` *(default)* | The interner hashes with [`ahash`](https://crates.io/crates/ahash) instead of std's `RandomState`, seeded per process. See below. |
| `ahash-compile-time-rng` | The same hasher seeded once per build, for WebAssembly and other targets without a random source. See below. |
| `trace` | Turns on `winnow/debug`, which traces every rule as it runs. |

**About `ahash`.** Both hashers are seeded per process, so neither can be
driven quadratic by chosen input; `ahash` is simply faster on the short keys an
interner sees - **15-28 % on the interner itself** (`benches/interning.rs`).
Inside a parse the context's lookup cache hides most of that, but a program
that looks names up *after* the parse does not go through the cache: Nikaia's
compiler, which resolves names in its checker and its prover, is **2.5-3.6 %
faster** as a whole with it. That is why it is the default.

**WebAssembly.** `ahash` gets its seed from `getrandom`, and `getrandom` does
not compile for `wasm32-unknown-unknown` unless the final application chooses
a backend. Two ways, pick one:

- Keep the default and choose the backend in the application, as `getrandom`
  intends: its `wasm_js` feature plus `--cfg getrandom_backend="wasm_js"` (see
  the [getrandom documentation](https://docs.rs/getrandom/latest/getrandom/#webassembly-support)).
- Or take the seed from the build instead:

  ```toml
  winnow-grammar = { version = "0.2", default-features = false, features = ["ahash-compile-time-rng"] }
  ```

  Every run of one binary then hashes alike, so someone who has the binary can
  work out colliding keys, and two builds of the same source are not
  byte-identical. For a parser that only reads its own user's input, neither
  matters. With both features on, the per-process seed wins.

`default-features = false` with neither gives std's `RandomState`, which builds
everywhere (on `wasm32-unknown-unknown` with fixed keys) and is the slower
hasher.

### Build one context, clone it

A context is cheap to build - **46 ns** - because the interner behind it and
the lookup cache in front of it are both created on first use, so a grammar
that never interns pays for neither. Cloning one is cheaper still, and it is
what you want anyway: a clone *shares the interner*, so symbols from two files
mean the same thing, which is what ADR 14 is for.

```rust,ignore
let context = ParseContext::<()>::default();      // once
for src in sources {
    let mut stream = ParseInput { input: LocatingSlice::new(src), state: context.clone() };
    let ast = Grammar::parse_file().parse_next(&mut stream)?;
    // …
}
```

`benches/context.rs` has the numbers, including what this used to cost: a
context was 1.35 µs before the interner was made lazy, almost all of it
allocating a sharded map that a non-interning grammar never read.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.

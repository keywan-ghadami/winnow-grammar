# Grammar Syntax Reference

This document serves as the reference for the **Grammar Definition Language** shared by all backends (`syn-grammar`, `winnow-grammar`).

## Defining Grammars

Grammars are defined using the `grammar!` macro. A grammar block contains a set of rules.

```rust
# use winnow_grammar::grammar;
# fn main() {
grammar! {
    grammar MyGrammar {
        start = "hello"
    }
}
# }
```

## Rules

A rule consists of a name, a return type, a pattern, and an action block.

```text
rule name -> ReturnType = pattern -> { action_code }
```

- **`name`**: The name of the rule.
- **`ReturnType`**: The Rust type returned by the rule.
- **`pattern`**: The grammar pattern to match.
- **`action_code`**: A Rust block that constructs the return value.

### Lexical vs. Syntactic Rules (Case Sensitivity)

The casing of a rule's name determines its whitespace handling:

- **Syntactic Rules (lowercase)**: Rule names starting with a **lowercase** letter (e.g., `rule expression`) allow implicit whitespace between patterns.
- **Lexical Rules (UPPERCASE)**: Rule names starting with an **uppercase** letter (e.g., `rule IDENTIFIER`) are **lexical**. They do **not** allow implicit whitespace between patterns.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
// Syntactic: matches "a + b"
 add = "a" "+" "b"

// Lexical: matches "ab", but NOT "a b"
 AB = "a" "b" 
#         }
#     }
# }
```

## Syntax Guide

### Sequences & Bindings
Match a sequence of patterns. Use `name:pattern` to bind the result to a variable available in the action block.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
 assignment -> (&'a str, i32) =
    name:raw_ident "=" val:i32 -> { (name, val) }
#         }
#     }
# }
```

### Alternatives
Match one of several alternatives using `|`. The first one that matches wins.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
 choice -> bool = 
    "yes" -> { true }
  | "no"  -> { false }
#         }
#     }
# }
```

### Repetitions
- `pattern*`: Match zero or more times. Returns a `Vec`.
- `pattern+`: Match one or more times. Returns a `Vec`.
- `pattern?`: Match zero or one time. Returns an `Option`.
- `pattern{n}`: Match exactly `n` times. Returns a `Vec`.
- `pattern{n,}`: Match at least `n` times. Returns a `Vec`.
- `pattern{n,m}`: Match between `n` and `m` times. Returns a `Vec`.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
 list -> Vec<i32> = elements:i32* -> { elements }
#         }
#     }
# }
```

**Bounded repetition.** `*` and `+` say *unbounded*. Where the format fixes a
width, say so — the parser then knows it, and a fixed-width format is parsed as
one rather than scanned:

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
// A temperature like `-12.3` or `4.5`: one or two whole digits, exactly one
// decimal. Parsed as tenths, so the arithmetic stays integral.
 TENTHS -> i32 =
    neg:"-"? whole:digit{1,2} "." frac:digit
    -> {
        let mut v: i32 = 0;
        for d in whole { v = v * 10 + (d as i32 - '0' as i32); }
        v = v * 10 + (frac as i32 - '0' as i32);
        if neg.is_some() { -v } else { v }
    }
#         }
#     }
# }
```

Bounds are **greedy and possessive**, like `*` and `+`: the repetition takes as
many elements as it can up to the upper bound and never gives one back to help a
later pattern match. At the upper bound it stops, and what follows sees the rest
of the input — `digit{2}` against `123` matches `12` and leaves `3`. Below the
lower bound the element's own error is the failure.

An upper bound below the lower one, and a bound that can match nothing (`{0}`,
`{0,0}`), are rejected where they are written.

> **Braces:** `{ pattern }` is still the braced-delimiter pattern. Only a brace
> group whose content **starts with an integer** is read as a bound, so
> `x { y }` is unchanged — but `x{2}` is now a repetition, and braces around a
> literal `2` are written `x "{" "2" "}"`.

### Delimiters
To match literal delimiters (parentheses, brackets, braces) in the input, use the specific delimiter syntax. This avoids ambiguity with grouping parentheses.

- `paren(pattern)`: Matches `( pattern )`.
- `[ pattern ]`: Matches `[ pattern ]`.
- `{ pattern }`: Matches `{ pattern }`.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
 tuple -> (i32, i32) = 
    paren(a:i32 "," b:i32) -> { (a, b) }
#         }

#     }
# }
```

Use standard parentheses `(...)` **only** for logical grouping of patterns (e.g., inside an alternative).

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
 group = ("a" | "b") "c"
#         }
#     }
# }
```

### Literals
Match specific tokens or text using string literals.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
 kw  = "fn" "name"
#         }
#     }
# }
```

For matching Rust literals as values, use the `lit_*` built-ins:
- `lit_str`: Matches a string literal.
- `lit_int`: Matches an integer literal.
- `lit_char`: Matches a character literal.
- `lit_bool`: Matches `true` or `false`.
- `lit_float`: Matches a floating-point literal.

### Built-in Primitives
The following primitives are "portable" and expected to be available in all backends, though their exact return types may vary slightly (e.g., `String` vs `syn::Ident`).

| Parser | Description |
|---|---|
| `ident` | An identifier (e.g., variable name). |
| `string` | A string literal (same as `lit_str`). |
| `u32` | Unsigned 32-bit integer. |
| `i32` | Signed 32-bit integer. |
| `bool` | Boolean (`true` or `false`). |
| `alpha` | Alphabetic characters. |
| `digit` | A single digit (`char`). |
| `digit1` | One or more digits (`&str`). |
| `whitespace` | Explicit whitespace matching. |
| `eof` | End of input. |

*Note: Backends may provide additional specialized built-ins.*

## Operators

### Cut Operator (`=>`)
The cut operator commits to the current alternative. If the pattern *before* the `=>` matches, the parser will **not** backtrack to other alternatives if the pattern *after* the `=>` fails.

```rust
# use winnow_grammar::grammar;
# #[derive(Debug)]
# pub enum Stmt<'a> {
#     Let(&'a str, Box<Expr>),
#     Expr(Box<Expr>),
# }
# #[derive(Debug)]
# pub struct Expr;
# fn main() {
#     grammar! {
#         grammar Test {
 stmt -> Stmt<'a> =
    "let" => name:raw_ident "=" e:expr -> { Stmt::Let(name, Box::new(e)) }
  | e:expr -> { Stmt::Expr(Box::new(e)) }

expr -> Expr = i32 -> { Expr }
#         }
#     }
# }
```

### Lookahead (`peek`, `not`)
- `peek(pattern)`: Succeeds if `pattern` matches, but does not consume input.
- `not(pattern)`: Succeeds if `pattern` does *not* match.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
check = "a" peek("b")
#         }
#     }
# }
```

### Lexical Control (`lex`, `spaced`)
- `lex(pattern)`: Forces a **lexical context** (no implicit whitespace) for the duration of the pattern.
- `spaced(pattern)`: Forces a **syntactic context** (implicit whitespace allowed) even inside a lexical rule.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
 word -> &'a str = lex(w:raw_ident) -> { w }
 CAST_OPERATOR = "as" spaced("<" T ">")
rule T = "bool"
#         }
#     }
# }
```

### Special (`until`, `count`, `eof`, `fail`, `recover`)

- **`until(terminator)`**: Consumes tokens until `terminator` is matched. The terminator is not consumed.
- **`count(pattern)`**: Returns the number of times `pattern` matched (as `usize`).
- **`eof`**: Succeeds only at the end of the input.
- **`fail("message")`**: Explicitly fails with a custom error message.
- **`recover(rule, sync)`**: If `rule` fails, skips input until `sync` token is found.
- **`fold(rule, init, step)`**: Repeats `rule` zero or more times, threading an
  accumulator instead of collecting. `init` is called once to build the starting
  value; `step` receives `(accumulator, item)` and returns the next accumulator.

  Use it wherever `rule*` would build a `Vec` you only intend to reduce — the
  collection is the memory cost on large inputs, not the parse. A data file with
  millions of records is summarised in constant space:

  ```rust
  # use winnow_grammar::grammar;
  # fn main() {
  grammar! {
      grammar Records {
          // (count, total) for the whole input; no Vec is ever built.
          pub summary -> (usize, i64) =
              s:fold(record, || (0usize, 0i64),
                     |acc: (usize, i64), v: i64| (acc.0 + 1, acc.1 + v))
              -> { s }

          rule record -> i64 = ident "=" v:i64 -> { v }
      }
  }
  # }
  ```

  Like `rule*`, a fold matches zero occurrences, so it succeeds on empty input
  and yields the initial accumulator. Bindings inside `rule` are consumed by
  `step` and do not escape to the surrounding action.

## Error Messages

Every generated parser reports failures as `winnow_grammar::ParseError`. The
message names what was expected, what was found, where, and in which rules:

```text
expected one of: `&`, identifier; found unexpected token `)` at line 1, column 9
in ty
in arg
in item 1
in decl
```

How the message is chosen, in this order:

1. **Progress** — of two failing branches, the one that got further wins.
2. **Priority** at the same position — `fail("…")` beats everything, a labelled
   alternative beats a bare token expectation.
3. Otherwise the expectations are **merged** into `expected one of: …`.

Failures that an optional (`x?`) or a repetition (`x*`) discards are remembered:
if the rule later fails at a shallower position, or input is left over, that
remembered reason is reported instead of a generic message.

Tools you have:

- `# "label"` after an alternative names it. If the alternative fails at its
  start, the label becomes the expectation (`expected one of: number, string`)
  instead of the internal token message.
- `fail("…")` reports the text verbatim, with high priority.
- `parse_next` returns the bare `ParseError` (use `e.render(source)` for the
  position); `.parse()` goes through winnow's own `ParseError`, which prints the
  position and the source line itself.
- The error is a value: `e.expected`, `e.found`, `e.rule_stack`, `e.offset`.

The contract, one test per point, is in `docs/adr/adr15-diagnostics.md`.

## Advanced Features

### Rule Arguments
Rules can accept arguments to pass context or configuration.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
main -> i32 = "start" v:value(offset=10) -> { v }
value(offset: i32) -> i32 = i:i32 -> { i + offset }
#         }
#     }
# }
```

### Generic Rules
Define reusable rules with generic types and parser parameters.

A rule can take **type** parameters (`<T>`) and **parser** parameters
(`(item)`). Parser parameters are substituted at each call site; the rule is a
template and is not compiled on its own. A type parameter is taken from the
call (`list<u32>(…)`) or, if omitted, inferred from the argument in the same
position (`list(item=u32)` gives `T = u32`).

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
list<T>(item) -> Vec<T> = items:item* -> { items }
integers -> Vec<i32> = l:list(item=i32) -> { l }
explicit -> Vec<i32> = l:list<i32>(item=i32) -> { l }
#         }
#     }
# }
```

Declaring the parameter as `item: Rule<T>` ties its result type to `T`
explicitly; both spellings are equivalent.

### Left Recursion
Direct left recursion is automatically detected and compiled into an iterative loop, making expression parsing natural.

```rust
# use winnow_grammar::grammar;
# fn main() {
#     grammar! {
#         grammar Test {
 expr -> i32 =
    l:expr "+" r:term -> { l + r }
  | t:term            -> { t }

term -> i32 = i:i32 -> { i }
#         }
#     }
# }
```

### Shadowing Detection
The compiler checks for unreachable alternatives (e.g., if a prefix shadows a longer rule) and emits warnings or errors.

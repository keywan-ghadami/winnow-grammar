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

An element that can match the empty input still owes the lower bound: `("a"?){3}`
matches the empty input three times, as `(a?){3}` does in a regex. Beyond the
lower bound an empty match ends the repetition rather than repeating forever.

An upper bound below the lower one, and a bound that can match nothing (`{0}`,
`{0,0}`), are rejected where they are written.

> **Braces:** `{ pattern }` is still the braced-delimiter pattern. Only a brace
> group whose content **starts with an integer** is read as a bound, so
> `x { y }` is unchanged. For the one group that is now ambiguous — braces
> around an integer literal — write the keyword form, `x brace(2)`, exactly as
> `( … )` takes `paren( … )` because the bare form is a group. A delimiter
> keeps its keyword form for as long as its bare form means something else;
> `[ … ]` is ambiguous with nothing and has none.

### Delimiters
To match literal delimiters (parentheses, brackets, braces) in the input, use the specific delimiter syntax. This avoids ambiguity with grouping parentheses.

`[ … ]` and `{ … }` are written bare; `( … )` is a group, so its delimiter form
is `paren( … )`. `{ … }` also has the keyword form `brace( … )`, needed only
when the content starts with an integer literal — the bare form would read as a
repetition bound.

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
| `ident` | An identifier (e.g., variable name), interned - a `Symbol`. |
| `raw_ident` | The same characters, borrowed from the input, not interned. |
| `intern(p)` | Runs `p` and interns its text: `Symbol`. See below. |
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

### Interning: `ident` and `intern(p)`

`ident` returns a `Symbol` - a 4-byte id for the text, taken from the
interner the `ParseContext` carries. Two occurrences of the same identifier
in one parse are the same `Symbol`, so a consumer compares ids instead of
strings; `ctx.interner.resolve(sym)` gives the text back.

`intern(p)` is the general form, and `ident` is exactly `intern(raw_ident)`.
The argument is an ordinary pattern whose output is text, so anything that
yields `&str` can be interned - a field value, a quoted string, a rule of
your own:

```rust,ignore
pub CITY -> Symbol = s:intern(until(";")) -> { s }
pub key  -> Symbol = s:intern(string)     -> { s }
```

`intern` is a `Symbol` factory, nothing more: it does not trim, lower-case or
otherwise normalise. An action block reaches the context directly as
`_state` when you need that - `-> { _state.interner.intern_string(&s.to_lowercase()) }`.
Every action gets `_state`, whether or not it names one; the `_` prefix keeps
an unused one quiet, and an action may bind the name itself, which shadows it.

**Reaching the user state: declare it.** A rule is generic over the state
type, so `_state.user_state` has type `S` in an action and nothing can be done
with it. A grammar that wants its own state says so - see
[Declaring a state](#declaring-a-state-state-t) below - and then writes
`_state.user()`.

**The symbol is a number you can use.** `Symbol::index()` is its position in
the interner: dense, zero-based, in the order the interner first saw each
string. The `n` distinct strings it holds have the indices `0..n`
(`InternerContext::len()`), so a caller's own table is a plain `Vec` addressed
in one step - no second lookup at aggregation time:

```rust,ignore
let i = sym.index() as usize;
if i >= totals.len() { totals.resize(i + 1, 0); }
totals[i] += temp;
```

The index means nothing outside the interner that produced it: it depends on
the order strings were first seen, so it differs between runs over different
input, between two interners over the same input, and - when the pieces of a
`par_fold` share one interner - on how the threads interleaved. Do not persist
it and do not read order into it. `resolve` is the way back to the text.

Two properties come from the interner rather than from `intern`. An
alternative that interns and then backtracks leaves its entry behind - no
symbol is ever wrong, the interner just holds more than the result names. And
a symbol is meaningful only against the interner that made it, which is what
the `par_fold` note below is about.

## Declaring a State: `state T;`

A grammar that has to keep something of its own while parsing - a symbol table
with scopes, an arena, a slot table whose numbers index an accumulator - names
the type once:

```rust,ignore
#[derive(Clone, Debug, Default)]
struct Seen { words: usize }

grammar! {
    grammar Counting {
        state Seen;

        pub word -> usize = w:alpha1 -> {
            _state.user().words += 1;      // `&mut Seen`
            w.len()
        }
    }
}
```

`_state.user()` is the accessor; it is a method rather than a second binding,
so it holds no borrow across the rest of the action and the context's own
fields (`_state.interner`, …) stay usable in the same action, in any order.

The declaration is a **bound, not a substitution**: a rule still takes any
state, and requires only that it provides a `Seen`. A state that *is* a `Seen`
satisfies that with nothing to write, and one state can satisfy several
grammars at once by implementing `StateOf` for each:

```rust,ignore
struct App { seen: Seen, names: Names }
impl StateOf<Seen>  for App { fn state(&mut self) -> &mut Seen  { &mut self.seen } }
impl StateOf<Names> for App { fn state(&mut self) -> &mut Names { &mut self.names } }
```

A parse then runs with `ParseContext::with_state(App::default())` and both
grammars find their part. A state that provides nothing of the sort is
rejected where it is passed, in the grammar's own words:

```text
error: the grammar declares `state Table`, but this parse's state does not
       provide one
   = note: parse with a state of type `Table`, or implement `StateOf<Table>`
           for the state you have
```

A grammar declares at most one `state`; a grammar that needs two things names
the type that holds both. A grammar that declares none is generic over the
state exactly as before - the declaration adds a bound and changes nothing
else, `parse_<rule>_pieces` and its `new_context` closure included.

**Testing.** `parse_test` is fixed to `ParseContext<()>` so that the common
test needs no turbofish; a grammar with a state uses `parse_test_in(state,
input)` beside it (`winnow_grammar::testing::WinnowTestExtWith`).

**What backtracking does to it.** Nothing, in the sense that matters: a branch
that writes to the state and then loses has still written. There is no
snapshot at alternative granularity, so state that a grammar writes wants to
be *idempotent* - assigning a slot twice yields the same slot, exactly as
interning does. A count or a sum belongs in what a rule returns and what
`fold` combines, where a lost branch's value is discarded with it; or after a
cut (`=>`), which no enclosing alternative retries past. A failed *parse* is a
different matter and is handled: under the default `Diagnose::Replay` the
state is restored before the diagnosing pass, so an action runs once.

## Hand-written Parsers: `extern rule`

A parser that is easier to write in Rust than in the DSL - a scanner of your
own, a token the grammar cannot express, a lookup in the interner - is
declared in the grammar and written next to it. It is called from generic
code, so it is generic over the state type too - which is exactly how it
reaches a declared state: bound it with `StateOf<T>` and call `.state()`.

```rust,ignore
fn city<'a, S>(i: &mut ParseInput<'a, S>) -> Result<usize, ParseError>
where
    S: Clone + std::fmt::Debug + StateOf<Slots>,
{
    let name: &str = take_till(1.., ';').parse_next(i)?;
    Ok(i.state.user_state.state().slot(name))
}
```

```rust,ignore
use winnow::token::take_till;
use winnow::Parser;
use winnow_grammar::{error::ParseError, grammar, ParseInput, Symbol};

// The signature: winnow's own, over `ParseInput`, returning this crate's
// `ParseError` - not `ErrMode<ParseError>`, and not the grammar's error type
// parameter. The generated code converts it (`Diagnostics::from_parse_error`)
// and drops it in the fast pass, so a hand-written parser costs nothing there.
fn city<'a, S: Clone + std::fmt::Debug>(i: &mut ParseInput<'a, S>) -> Result<Symbol, ParseError> {
    let s: &str = take_till(1.., ';').parse_next(i)?;
    Ok(i.state.interner.intern_string(s))   // the context is reachable here
}

grammar! {
    grammar Cities {
        // Declares that `city` exists and what it returns. Without this the
        // validator rejects the call as an undefined rule.
        extern rule city -> Symbol;

        pub row -> (Symbol, i32) = c:city ";" t:i32 -> { (c, t) }
    }
}
```

The declaration is `extern rule name -> Type;`, optionally with parameters
(`extern rule pair(a, b) -> (A, B);`) and generics. It only tells the validator
the name exists; the call site emits a plain path, so the function has to be in
scope where the `grammar!` macro is.

A hand-written parser runs in both passes of ADR 17 (the fast one and the
diagnosing replay), so the same rules apply to it as to an action: it may
intern freely - that is idempotent - and anything else it mutates has to
tolerate running twice.

Under a `#[frame]` the check cannot see into it, so whether it is safe to cut
around is yours to know - see *Where the check stops* under Frames.

## Backends

The DSL is shared with other backends in principle - the model crate
(`winnow-grammar-model`) parses and validates a grammar without knowing which
one generates code. In practice **`winnow-grammar` is the only backend, and
the DSL currently assumes it.** The clearest place this shows is the built-ins
that take a positional argument: which names those are is a fixed list in the
grammar parser (`separated`, `repeated`, `intern`), because parsing runs
before the backend is known. A second backend with a different set would need
that list to come from the backend - an arity on `BuiltIn` and a parse that
carries it (`feature-requests.md` §1). Until such a backend exists, the list
stays where it is.

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

- **`until(terminator)`**: Consumes input until `terminator` is matched, and **returns the text it skipped** (`&'a str`). The terminator is not consumed. With no terminator in the rest of the input it consumes to the end rather than failing.
- **`count(pattern)`**: Returns the number of times `pattern` matched (as `usize`).
- **`eof`**: Succeeds only at the end of the input.
- **`fail("message")`**: Explicitly fails with a custom error message.
- **`recover(rule, sync)`**: If `rule` fails, skips input until `sync` token is found.
- **`par_fold(rule, init, step, merge)`**: `fold` plus a merge, over a `#[frame]`
  rule — see **Frames and parallel parsing** below.
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

## Frames and parallel parsing

A parser reads its input front to back on one core. For a large input that is
the whole cost. A grammar can say the two things that let the input be cut into
pieces and parsed as pieces. The decision record is `docs/adr/adr16-frames.md`.

**Where may it be cut?** A rule marked `#[frame]` is one that can be found
from any offset by scanning to the next **boundary** — the literal it ends in,
or the one named in `#[frame(boundary = "\n")]`:

```rust
# use winnow_grammar::grammar;
# fn main() {
grammar! {
    grammar Measurements {
        // Up to `;` - or to the end of the frame, whichever is first. Under a
        // frame the terminator must cover the boundary, and it says so:
        // `frame_end` is the boundary of the frame this rule is reached from,
        // written once in the attribute and referenced here.
        NAME -> &'a str = s:until(";" | frame_end) -> { s }

        #[frame(boundary = "\n")]
        pub MEASUREMENT -> (&'a str, i32) =
            name:NAME ";" temp:i32 frame_end -> { (name, temp) }

        pub FILE -> i64 =
            s:par_fold(MEASUREMENT, || 0i64,
                       |acc: i64, (_, t): (&str, i32)| acc + t as i64,
                       |a: i64, b: i64| a + b)
            -> { s }
    }
}
# }
```

**This is checked, not believed — and never repaired.** Cutting at the next
boundary is only right if the boundary cannot occur *inside* a frame. Every
rule reachable from the frame rule is walked, and what consumes input is one
of two cases:

- **Safe:** a literal without the boundary in it; a built-in whose alphabet
  cannot include it (`digit1`, `ident`, …); lookahead, which consumes nothing;
  an `until(…)` whose terminator **covers** the boundary — one of its
  alternatives is `frame_end`, the boundary literal itself (or a prefix of
  it), or `line_ending` for a newline boundary.
- **Rejected**, with the rule and pattern named: a literal that contains the
  boundary (a quoted-string rule that allows `"\n"` — CSV with quoted newlines
  is the classic case); a built-in that can consume it (`any`, `multispace0`);
  a **syntactic (lowercase) rule**, whose implicit whitespace is `multispace0`
  and eats newlines; an `until(…)` that does not cover the boundary — the
  message says what to add; and `recover(…)`, whose skip cannot be kept
  inside a frame. Recover per frame with an alternative instead:
  `(item | until(frame_end)) frame_end`.

Nothing here changes what a pattern means. `until(";")` consumes up to the
next `;` wherever it is written; inside a frame that is a mistake, and the
checker says so. The parser generated for a rule is the same whether or not a
frame reaches it.

**`frame_end`** is a built-in that stands for the boundary of the frame the
rule is reached from: as a parser it matches the boundary, as an alternative
of an `until` terminator it covers it by construction. It resolves statically
— a rule that writes it must be reached from a frame, and from frames of one
boundary only; both are compile errors otherwise. Writing the literal instead
(`until(";" | "\n")`) is allowed and means the same; `frame_end` keeps the
boundary in one place.

A frame must **end in its boundary** — `frame_end`, the literal, `line_ending`
for a newline boundary, `eof`, or a group of those — so that every frame ends
exactly at a boundary and the piece that finds a frame's end is the one that
owns it. A bare `#[frame]` infers the boundary from a trailing literal; a
frame ending in `frame_end` names it in the attribute.

`#[frame(boundary = "\n", unchecked)]` skips the walk: you assert the
invariant, as with `unsafe`, and the word is there to grep for. It is the
escape hatch for the formats the check cannot see through yet (below).

**How do two pieces combine?** `par_fold(rule, init, step, merge)` is `fold`
plus the merge, and requires `rule` to be a frame. It must be the whole body of
its rule: nothing before or after it, since a prefix or suffix would belong to
no piece. For the same reason its parser skips **no whitespace at its entry**,
unlike every other rule's: the parser runs once per piece, and whitespace
skipped there would be skipped at every cut rather than once at the start of
the input. A frame that begins with a space keeps the space; whitespace-only
text between two frames is a failure, in pieces and in one go alike.

**Where the check stops.** It reasons about what the grammar says. A parser it
cannot see into — a hand-written one reached by path (`super::word`), an
`extern rule`, or any bare name in a grammar with a glob `use …::*` — is not
walked, because whether it can consume the boundary is a fact about Rust the
grammar does not contain. **For those the guarantee is yours, not the
checker's:** a `par_fold` over a frame that reaches one is exactly as sound as
that parser is. There is no key to declare it safe, because such a key would
read like a checked claim while being an unverifiable promise; `unchecked` on
the frame already says "the author asserts this", and says it greppably.

A rule that only `peek(…)`/`not(…)` reaches is not checked against the
boundary: lookahead consumes nothing, so nothing it contains can carry the
parser past one.

**What is generated.** The grammar gets, next to the parsers:

- `frames_<RULE>(input, n) -> Vec<Range<usize>>` on a frame rule and on a
  `par_fold` rule: the byte ranges of `n` pieces. The input is divided into
  `n` equal byte ranges blind; every piece but the first then moves its start
  to just past the first boundary at or after that point, and ends where the
  next piece starts. Every frame lies in exactly one piece; a frame longer than
  a piece leaves the pieces after it empty; input without a trailing boundary
  keeps its last frame; every range is a UTF-8 character boundary, because a
  valid UTF-8 boundary can only match at one.
- `merge_<RULE>(a, b)` on a `par_fold` rule: the merge it was given.
- `parse_<RULE>_pieces(input, new_context, how)` on a `par_fold` rule: the
  driver. It cuts, parses every piece with `parse_<RULE>()` under a context
  from `new_context`, merges first piece first, and reports a failing piece's
  error at its offset in the whole input. `how` is
  `winnow_grammar::rt::Parallelism`:

  | `how` | what runs |
  |---|---|
  | `Parallelism::Off` | no cut, one parse — the same as `parse_<RULE>()` |
  | `Parallelism::Pieces(n)` | `n` pieces |
  | `Parallelism::Auto` (default) | one piece per available core |

  With the crate's `rayon` feature the pieces run on rayon's global thread
  pool (size it with `rayon::ThreadPoolBuilder`); without it the same driver
  runs them in sequence — the same cut and the same answer, which is what lets
  a test check the split without threads.

```rust,ignore
use winnow_grammar::{rt::Parallelism, ParseContext};

let context = ParseContext::<()>::default();
let total = Measurements::parse_FILE_pieces(&input, &context, Parallelism::Auto)?;
// or, with an executor of your own: frames_FILE + parse_FILE() + merge_FILE
```

The sequential `parse_FILE()` over the whole input gives the same answer, which
is what `tests/frames_test.rs` asserts — on inputs it accepts and on inputs it
rejects.

**Every piece parses with a clone of the context**, so the interner inside it
is shared and symbols from two pieces mean the same thing. That is the plain
call above, and it needs no thought.

`parse_<RULE>_pieces_with` is the other answer: it *builds* a context per piece
from a closure you give it, for a `user_state` that has to start empty in every
piece - a slot table whose numbers are the piece's own, an accumulator that
must not be copied. For that workload it is the ordinary entry point, not an
exception:

```rust,ignore
let totals = Measurements::parse_FILE_pieces_with(
    &input,
    || ParseContext::with_state(Table::default()),
    Parallelism::Auto,
)?;
```

Note what that closure decides. A context built fresh per piece has a fresh
*interner* too unless you clone one in, and symbols from two pieces are then
not comparable: two different words can share an id, one word can have two,
and nothing fails. Clone an interner into the closure when symbols leave their
piece:

```rust,ignore
let interner = InternerContext::new();
let make = { let interner = interner.clone(); move || ParseContext::with_state_and_interner(Table::default(), interner.clone()) };
```

The same is true of anything else keyed per piece: a table's slot numbers are
its piece's, exactly as symbols are their interner's. And a merge cannot repair
it - `merge` is handed two values, and by then the piece's context is gone. So
a number that has to mean something in another piece comes from something the
pieces share:

```rust,ignore
// A table shared by every piece, the way the interner is shared - the slots
// are then global, at the price of the lock.
let table: Arc<Mutex<Table>> = Arc::default();
let make = { let table = table.clone(); move || ParseContext::with_state(table.clone()) };
```

The alternative - a table per piece, merged by name - is faster and is not
expressible today; `docs/adr/adr21-merging-across-pieces.md` says what it would
take. `tests/shared_interner_test.rs` shows both halves of the interner case
side by side.

**What a frame cannot say yet.** A boundary is a byte string. A format whose
cut points need more than a search — CSV with quoted newlines (quote parity),
records recognisable by how they *start*, escaped boundaries, a scanner of
your own — is not expressible, and `unchecked` is the way to parse it until it
is. The attribute is a keyed list so that each of those is another key when it
comes (`quote = …`, `start = …`, `escape = …`, `scan = …`); the positional
forms `#[frame = "\n"]` and `#[frame("\n")]` are rejected for that reason.

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

### When the diagnosis is made

None of the above is paid for by a parse that succeeds. Every entry point
first runs a **fast pass** with a zero-sized error type - no expectations,
positions or rule stacks are built - and only a failure is parsed a second
time with the full engine, whose error is then reported. The messages are
the same; the successful parse is cheaper. On a `par_fold` rule the second
pass starts at the item the first one stopped in, so a bad line in a large
file costs one item to explain, not the file.

`ParseContext::diagnose` chooses the mode:

```rust,ignore
let mut ctx = ParseContext::<()>::default();
ctx.diagnose = Diagnose::Off;   // the verdict alone: no second pass, no position
```

| `Diagnose::…` | on failure |
|---|---|
| `Replay` (default) | restore `user_state` from a clone taken before the fast pass, then diagnose |
| `ReplayInPlace` | diagnose on the context as the fast pass left it - no clone, an action that mutates `user_state` runs twice |
| `Off` | no second pass; the error `is_undiagnosed()` and carries no position |
| `Eager` | no fast pass at all: diagnose straight away |

What this asks of a grammar: an action that mutates `user_state` must
tolerate being replayed after a failure - under `Replay` on the restored
state, provided `S::clone` is a snapshot (a state that shares through `Arc`
is not restored by cloning). Interning is idempotent and needs no care. The
reasoning and the contract are in `docs/adr/adr17-lazy-diagnostics.md`.

## Advanced Features

> **How `until` and `recover` skip.** Where the terminator's match is a fixed
> string the skip is a **scan** (via `memchr`: SIMD where the target has it,
> the same word-at-a-time trick in portable code where it does not). Any other
> terminator has to be *tried* at every position, a parser call per character;
> that path yields the same value, it is only slower — and it is slower in
> proportion to the field, so it grows with the input rather than sitting at a
> constant factor. Measured on 2000 rows of `name;digits`: 2.7x with
> eight-character names, 7.3x with forty-character ones.
>
> **A terminator's match is a fixed string when it is** a string or char
> literal; the built-in `line_ending` or `eof` (`until(eof)` is simply the rest
> of the input); `frame_end`, which stands for the enclosing frame's boundary;
> **a lexical rule of your own that matches nothing but literals** — directly
> (`SEP -> () = ";"`), through alternatives (`SEP -> () = ";" | "|"`), or
> through a chain of such rules; or a group of alternatives **all** of which
> are one of these. One alternative that is not drops the scan for the whole
> group.
>
> Two exclusions are worth knowing because neither is visible in the grammar:
>
> * **A syntactic rule does not qualify**, even with the same body. A
>   syntactic rule skips whitespace before its elements, so `sep -> () = ";"`
>   matches `  ;` — it begins where the whitespace begins, not where the
>   literal is, and `until(sep)` therefore stops in a different place than
>   `until(";")` would. That is a difference in *meaning*, not in speed, so it
>   is kept. Name the rule in uppercase (`SEP`) when you want the literal and
>   the fast path.
> * A rule of your own named `line_ending` or `eof` is that rule, not the
>   built-in; it qualifies only under the same conditions as any other rule.

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

The names `'a`, `S` and `E` are taken: the generated code uses them for the
input lifetime, the user state and the error type of a rule.

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

# Feature Requests for `syn-grammar`

This document outlines feature requests and issues encountered while building a
TOML parser and safety-critical tests using `winnow-grammar` (which acts as a
backend for `syn-grammar-model`).

**Status, 2026-09-09.** The requests below are kept as they were written; each
now carries what happened to it, checked against the current tree rather than
remembered. Five of the six are answered - two by building the thing, two by
deciding against it and saying so where a grammar author looks, one by a
restriction that has since been lifted. A document that still asks for what
exists costs more than one that is empty, which is why this pass happened at
all: twice this week a stale sentence sent someone down the wrong path.

| | request | status |
|---|---|---|
| 1 | arguments for built-ins | partly, deliberately |
| 2 | extern / imported rules | **built** |
| 3 | `!` and `&` operators | decided against; `not(…)`/`peek(…)` are the form |
| 4 | `ident (group)` read as a call | **fixed** |
| 5 | `fail` built-in | **built** |
| 6 | type-inference help | no case on file |

## 1. Support Arguments for Built-in Rules

**Context:**
We attempted to implement a `take_until` built-in to handle comments efficiently. We added it to the `get_builtins()` list in the backend.

**Issue:**
`syn-grammar-model`'s validator explicitly forbids arguments for rules defined as built-ins.
```rust
// In validator.rs
if is_builtin && !args.is_empty() {
    return Err(syn::Error::new(..., "Built-in rule '...' does not accept arguments."));
}
```

**Request:**
Allow built-ins to accept arguments.
*   **Option A:** Relax validation to allow any number of arguments for built-ins (deferring checking to the backend).
*   **Option B:** Update `BuiltIn` struct to include an optional arity or signature definition.

**Example Use Case:**
```rust
rule comment -> () = "#" take_until("\n") -> { () }
```

> **Status: partly, and the rest deliberately.** A built-in *can* take a
> positional argument - `intern(p)`, `text(p)`, `dec<T>(p)`, `separated`,
> `repeated` all do. Which names those are is a fixed list in the grammar
> parser, because parsing runs before the backend is known (`parse_grammar` is
> generic over `B`, `syn::parse2` is not), so Option B - an arity on `BuiltIn`,
> resolved from the backend's own list - is what remains unbuilt. It stays
> unbuilt while there is one backend: the general machinery would serve a
> second one that does not exist. SYNTAX.md's *Backends* section says this
> plainly rather than leaving it to be discovered.

## 2. Support for "Extern" or Imported Rules

**Context:**
We wanted to define a complex parser logic (like whitespace handling or specific token consumption) in standard Rust functions and use them inside the grammar. While the `grammar!` macro allows `use` statements, the validator flags any rule name not defined in the `grammar` block as "Undefined rule", even if it is imported.

**Issue:**
Users cannot easily mix generated rules with handwritten parsers without hacking the backend's "built-in" list.

**Request:**
Add syntax to declare external rules that should skip existence/definition validation.

**Proposed Syntax:**
```rust
grammar MyGrammar {
    use super::my_custom_parser;
    
    // Tell syn-grammar that this exists externally
    extern rule my_custom_parser; 

    rule main -> () = my_custom_parser -> { () }
}
```

> **Status: built**, in almost exactly that shape -
> `extern rule name(params) -> Type;`, with generics and attributes. The
> signature it needs is winnow's own over `ParseInput`, returning this crate's
> `ParseError` rather than `ErrMode<ParseError>`; SYNTAX.md's *Hand-written
> Parsers* section documents it and `tests/extern_rule_test.rs` pins it. Note
> what it does not buy: it is called from generic code, so it is generic over
> the state type too - reaching a concrete user state is what `state T;` is
> for.

## 3. Syntax for `Not` (`!`) and `Peek` (`&`) Operators

**Context:**
The `syn-grammar-model` AST (`ModelPattern`) contains `Not` and `Peek` variants. However, attempting to use standard EBNF/PEG syntax fails at the parsing stage.

**Issue:**
Input: `! line_ending`
Error: `error: expected ident`

**Request:**
Implement parsing support for negation and lookahead.

**Proposed Syntax:**
```rust
rule no_newline -> char = !"\n" c:any -> { c }
// OR
rule no_newline -> char = not("\n") c:any -> { c }
```

> **Status: the second form, and the first is refused on purpose.** `not(p)`
> and `peek(p)` are the syntax. `!` and `&` are recognised by the parser only
> to reject them with the form to write instead:
>
> ```text
> error: The '!' operator is not supported. Use 'not(pattern)' for negative
>        lookahead.
> ```
>
> That is a decision rather than a gap - the keyword form reads the same in a
> grammar that also writes `until(…)`, `recover(…)` and `intern(…)` - and it
> costs a reader nothing, because the error says what to write.

## 4. Resolve Parsing Ambiguity: `ident (group)` vs `call(args)`

**Context:**
In EBNF, whitespace is significant for separation, but `syn-grammar` seems to interpret an identifier followed by a parenthesized group as a function call, regardless of whitespace.

**Issue:**
```rust
rule ws -> () = 
    multispace0 (comment multispace0)* -> { () }
```
This fails with: `Rule 'multispace0' expects 0 argument(s), but got 1.`
The parser interprets `multispace0 ( ... )` as `multispace0(arg)`.

**Workaround used:**
We had to wrap the identifier in a group to force it to be treated as a pattern unit:
```rust
rule ws -> () = 
    (multispace0) (comment multispace0)* -> { () }
```

**Request:**
Improve the parser to distinguish between sequence and calls, potentially by requiring no whitespace for calls (if possible in `syn`) or providing a clearer sequence structure. At a minimum, this behavior should be documented.

> **Status: fixed.** An identifier followed by a parenthesised group is a
> *sequence*, unless the identifier is one of the built-ins that take a
> positional argument or the parentheses contain a named argument
> (`name = value`). `multispace0 (comment multispace0)*` parses as the
> sequence it looks like, with no wrapping group needed. This is the same
> fixed list as request 1, seen from the other side: it is what makes the
> ambiguity decidable without knowing the backend.

## 5. `fail` Built-in

**Context:**
For error recovery and control flow (like `recover(fail, sync)`), a parser that always fails is very useful.

**Request:**
Add `fail` to the standard set of built-ins or allow the backend to define it easily without it being flagged as undefined. (Solved partially by Request #1 and #2).

> **Status: built.** `fail("message")` is a keyword of the DSL, yields `()`,
> and its message wins against competing expectations at the same position by
> priority - `tests/diagnostics.rs` covers exactly that ranking.

## 6. Type Inference Improvements

**Context:**
When using combinators like `recover` or `alt` generated by the backend, Rust often fails to infer types if the return values are complex (e.g., `Result<String, _>` vs `Result<(), _>`).

**Request:**
While this is largely a backend codegen issue, `syn-grammar` could potentially allow explicit type annotation on sub-patterns or variable bindings to help the backend generate fully typed code.
```rust
// Hypothetical syntax
rule example -> () = x:(i32: u32) -> { ... }
```

> **Status of request 6: no case on file.** Nothing in the test suite currently
> fails to infer, and the two additions that came closest to the complaint -
> `dec<T>(p)`, whose output type is given by the call generics, and `text(p)`,
> which is always `&'a str` - removed the shapes most likely to produce it. An
> annotation syntax for sub-patterns is not proposed on the strength of a
> report that predates them; a grammar that still hits it is what would
> justify one, and would be welcome here.

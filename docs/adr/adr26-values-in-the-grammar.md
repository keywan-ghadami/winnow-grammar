# ADR 26: Values in the Grammar - Arguments, Bounds and Guards

**Status:** Accepted. **Date:** 2026-10-28.
**Tests:** `tests/values_test.rs`, `tests/yaml_indent_test.rs`.
**Raised by:** a YAML parser being written on this crate. YAML's block
structure is indentation, and its specification is written as productions
that take the indentation as a parameter: `s-indent(n)`, `l+block-mapping(n)`,
`s-l+block-node(n,c)`.
**Related:** ADR 20 (the state, and what backtracking does to it), ADR 24 §8
(runs of fixed shape), ADR 25 (byte input).

## Context

A rule can already take a typed parameter (`value(offset: i32)`), but a call
can pass it only an integer or `bool` literal. A name in an argument is read as
a rule (`Undefined rule: 'm'`), a repetition bound is a literal, and nothing in
a grammar can fail on a condition. So the indentation of the line being parsed
cannot reach the rule that parses the lines under it, and a format with a
length in front of its payload (`n:u16 data:byte{n}`) cannot be written at all.

The alternative that other tools take is a stack in the parser's state (pest's
`PUSH` and `PEEK`). It does not fit here: a write to the state is not undone
when an enclosing alternative is retried (ADR 20; SYNTAX.md, *What
backtracking does to it*), so an indentation pushed by a branch that lost would
stay pushed. A value passed as an argument is a function argument of the
generated parser: a branch that loses takes it with it, and backtracking is
correct without anything to restore. Python's other answer, a tokenizer that
emits INDENT and DEDENT, does not fit YAML either: `- - a` nests two sequences
on one line, and a block scalar's indentation is `n+1+m` with `m` detected or
given by an indicator.

## Decision

### D1 - A value in scope is a value

A **value** is a typed parameter of the rule being written - one whose type is
not `Rule<…>` (that is a parser parameter, of a template) - or a
binding of an element written earlier in the same sequence, or in a sequence
around it. Bindings inside an alternative are not in scope after it.

Where a value's name appears in a call argument or a repetition bound, the
argument or the bound is a **Rust expression**, evaluated where the parser
reaches it. A value shadows a rule of the same name there.

### D2 - Arguments

```rust,ignore
pub DOC -> Node = v:BLOCK(n = -1, c = Ctx::BlockIn) -> { v }
BLOCK(n: isize, c: Ctx) -> Node = m:count(" ") if((m as isize) > n) v:CONTENT(m = m as isize) -> { v }
```

An argument that names a value as an operand is parsed as an expression up to
the next `,` (`block(n + m)`, `entries(n, c)`, `(n - 1)`) and passed to the
rule's parameter as it is: the parameter's type decides, `rustc` checks. Such
an argument list is an argument list even when positional, where a bare
`name(…)` is otherwise a rule followed by a group. A value inside a pattern's
own bound or call - `text(byte{n})`, `wrap(p = inner(m))` - leaves that
pattern a pattern.

A named argument for a typed parameter of a rule of the same grammar is a
value whatever it could otherwise be read as: `c = Ctx::BlockIn` is the enum,
not a call of a rule `Ctx::BlockIn`, and `n = -1` is the number. That is
decided once every rule is known. Otherwise an argument that mentions no value
is what it was - a pattern, or a literal for a typed parameter.

A value passed where the rule expects a parser, or to a built-in that wraps a
pattern (`text(p)`, `dec(p)`), is an error at the call. A value given to a
typed parameter of a template is bound under the parameter's name before the
inlined body. Arguments are matched to parameters by position, as before.

### D3 - Bounds

```rust,ignore
SPACES(n: usize) -> () = " "{n} -> { () }
FEWER(n: usize) -> () = " "{0, n - 1} -> { () }
PAYLOAD -> &'a [u8] = n:u16 data:bytes(byte{n as usize}) -> { data }
```

A brace group after a pattern is a bound when its content starts with an
integer, as before, **or with a value**. Each bound is an expression of any
integer type. The repetition takes `k` elements with `min <= k <= max`; a
bound below zero counts as below every `k`, so `{0, n - 1}` with `n = 0`
matches nothing, rather than wrapping. No upper bound (`{n,}`) is unbounded.

What the analyses know about a bound they cannot evaluate: its minimum is 0
and its maximum open, so the nullability, first-byte and frame checks stay
sound (they over-approximate). A repetition with a computed bound is never
part of a run of fixed shape (ADR 24 §8), and a literal bound is unchanged.

### D4 - Guards

```rust,ignore
m:count(" ") if(m > n) …
c:ctx if(c == Ctx::BlockIn, "a block context") …
```

`if(cond)` matches nothing and succeeds when `cond` is true; otherwise it
fails where it stands, without consuming, as a backtrack - so the next
alternative is tried. The failure's expectation is the message when one is
given, and otherwise the condition as written. It is an ordinary element of a
sequence: after a cut it is a cut like any other.

To the analyses a guard is the empty pattern: nullable, nothing in its first
set, nothing for a frame to check.

## Consequences

* A grammar that uses none of it generates what it generated before: every
  argument and bound that mentions no value is parsed as it was.
* A value is a plain argument of the generated function. A rule with value
  parameters is called, never inlined as a template, and its entry point
  `parse_<rule>(…)` takes the values.
* An expression is Rust inside the grammar, as an action is: it is checked by
  `rustc`, and its errors point into the grammar.

## Not decided here

* Linear-time memoisation keyed by position. Nothing memoises rules today; a
  memo would have to key by the values too.
* Left recursion through a rule with value parameters. Nothing here makes it
  work, and nothing tests it.

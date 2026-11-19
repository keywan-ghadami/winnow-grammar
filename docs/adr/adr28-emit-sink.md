# ADR 28: `emit` - Values from Any Action to a Stream's `each`

**Status:** Accepted, implemented. **Date:** 2026-11-18.
**Tests:** `tests/stream_emit_test.rs`; `tests/stream_each_test.rs` and the
other stream tests of ADR 27 unchanged.
**Raised by:** Nikaia issue #2155: a format whose interesting values lie below the fold's items - an
event inside a document, a warning about one field - should not have to
build them into the item and take them apart again in `each`.
**Related:** ADR 20 (the state is not rolled back), ADR 27 §3 (the stream
driver, `#[stream(each)]`).

## Context

`#[stream(each)]` (ADR 27 §3) hands the caller every item of the fold at the
top of the path, by reference, inside the item step where `commit` holds.
Everything below an item reaches the caller only through the item's value.
An action sees one thing beside its bindings: the context, `_state:
&mut ParseContext<S>`. So the way from a nested action to `each` runs
through the context.

What makes it more than a field: the stream driver runs a step more than
once. An attempt that touches the end of its window is parsed again with
more input; a failure is parsed again to diagnose it (`Pass::diagnose`); an
error further on replays the item before it (`record_chunk`, `replay_prev`,
`Pass::replay`). An action runs in every one of these. `each` must see what
the parse emitted once, and only for what the parse took.

## Decision

### 1. `ParseContext::emit`

```rust
impl<S> ParseContext<S> {
    pub fn emit<T: Any + Send>(&mut self, value: T) -> bool;
}
```

An action calls `_state.emit(value)`. `T` is the fold's item type with
`'static` for its lifetimes. `each` keeps its signature,
`impl FnMut(&Item) -> ControlFlow<B>`; an emitted value is shown to it as
`&Item`, beside the items. `emit` returns whether an `each` will see the
value: `false` in every parse but a `parse_<rule>_reader_each`.

### 2. The slot: a type-erased buffer, checked on `push`

`ParseContext` gains a hidden field `emits: EmitSink`:

```rust
pub struct EmitSink {
    open: Option<Box<OpenSink>>,   // None while closed
}
struct OpenSink {
    item: TypeId,                  // the item type
    name: &'static str,            // and its name, for the panic
    pending: Vec<Box<dyn Any + Send>>,
}
```

Boxed so that the field is one pointer: the context is moved into and out
of the stream's window on every step, and a closed sink - every parse but a
`_reader_each` - costs one null check where the driver discards.

Closed by default and in every clone; `parse_<rule>_reader_each` opens it on
the stream's copy of the context with the item type (lifetimes made
`'static` by the generator), and `Drop for Stream` closes it before the copy
goes back to the caller's context, so a context never carries an open sink
past the parse. `push` on a closed sink drops the value; on an open one it
checks the `TypeId` and panics with both type names on a mismatch, so the
downcast in the generated code cannot fail.

Why erased: a typed slot needs the item type in the context's type -
`ParseContext<S, E>` - and every user of the context, every hand-written
parser and every helper generic over `S`, would have to name it. The erased
slot costs a box per emitted value and one `TypeId` compare, only where a
grammar emits.

Why `'static`: a value is held past the window it was parsed from (until its
attempt is taken) and is told apart by `TypeId`, which only `'static` types
have. `&Item<'static>` coerces to the `&Item<'w>` that `each` takes as long
as `Item` is covariant in its lifetimes, as a type built of references,
owned data and such types is. An item that borrows the input
(`(&'a str, i64)`) can be emitted only as one that borrows nothing.

Why `Send`: `ParseContext<S>` is `Send` where `S` is; a `Box<dyn Any>` would
take that away. `Sync` is kept by an `unsafe impl Sync for EmitSink`:
every method that reaches a value takes `&mut self`, `Clone` builds an empty
sink and `Debug` prints the count, so a `&EmitSink` reaches no value - the
argument of `std::sync::Exclusive`.

### 3. Held per attempt, handed on where the attempt is taken

A value goes into `pending` and waits there. The generated
`_reader_each` drains it into `each` at exactly the points where the parse
takes what it parsed:

* **an item**: inside the item step, in the `commit` branch, before the
  item's own tap - so a value emitted inside an item comes before it;
* **a step before or after the fold** (the prefix and suffix chunks of
  ADR 27 §3): after `__stream.chunk(…)?` returned;
* **the rule's action at each level of the path**: after it returned.

The driver drops `pending` wherever it throws an attempt away: before each
attempt (`run`, `attempt`), when an item attempt ends the fold,
before the fallback diagnosing attempt, and after the item loop. The
diagnosing replays set the pending values aside and put them back
(`EmitSink::set_aside`/`restore`), since they parse again what was already
handed on. Not in the item loop of `scan_window`, the hot path of every
stream: there nothing is held - a taken item handed its values over inside
`f`, and an attempt that is not taken ends the loop, after which `items`
discards (a `debug_assert` checks it). A discard per item cost every
`#[stream]` reader, `each` or not, ~39 instructions per item. These points overlap - most are guarded twice - and the tests fail
when they are removed together, and when the first-pass discard in `run` or
the set-aside of the replays is removed alone.

A `Break` from `each` on an emitted value ends the parse as a `Break` on an
item does: inside the item step it is `Fail::Stop` with the value kept in
`__broken`; between steps the generated code returns `Ok(Break(b))` at once.
In both cases nothing after it is read or checked.

### 4. What stays as it was

* Only `#[stream(each)]` rules generate the opening of the sink and the
  drains. Every other grammar, and a `#[stream]` rule without `each`,
  generates the code it did; its emits find a closed sink.
* What an action emits inside an item is what it writes to the state
  (ADR 20, SYNTAX.md *What backtracking does to it*): a branch that emits and
  then loses has still emitted, once its item is taken. Holding values per
  alternative would need a mark on every choice point of every grammar, the
  cost ADR 20 refused for the state.

## Considered and not done

* **A typed parameter** (`ParseContext<S, E>`, or the sink in `S`): breaking
  for every user of the context, for a feature few grammars use; the state
  route also makes the user write the drain.
* **Calling `each` from `emit` at once** (a `&mut dyn FnMut` in the
  context): unsound under the driver's retries - a value from an attempt
  that is parsed again, or diagnosed, would be seen twice - and the closure's
  lifetime would have to enter the context's type.
* **A separate emit type** (`each_emit: impl FnMut(&E)`): a second closure
  and a second type parameter on every `_each`, where the items and the
  values are usually one event type; a grammar that wants two kinds makes
  the item an enum.

Found on the way and fixed: an action of a `#[stream]` rule could not name
`_state` in the reader functions - the binding was spanned `mixed_site` and
so invisible to the user's tokens. It is now spanned at the call site, as in
memory.

## Consequences

* `ParseContext::emit` is new public API; `ParseContext` gains a hidden
  field `emits` and the hidden type `EmitSink`. A struct literal of
  `ParseContext` without `..Default::default()` stops compiling, as with the
  hidden fields before it; nothing else changes for a caller.
* User documentation: SYNTAX.md, *Values from any action: `emit`*, after
  `#[stream(each)]`.
* Where it lives: `src/winnow_grammar.rs` (`emit`, `EmitSink`),
  `src/rt/stream.rs` (the discards, `Drop`),
  `winnow-grammar-macros/src/codegen/stream.rs` (`emitted_type`,
  `deliver_emits`, `each_tap`, `stream_each_fn`).

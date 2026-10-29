#!/bin/sh
# Runs the test suite with every grammar's parsers wrapped in `rt::edge`, as a
# grammar with a `#[stream]` rule is generated (ADR 27 §3). The wrapping only
# notes where a parse touched the end of its input, so every result must stay
# what it is; a separate target directory keeps the macro's expansion apart
# from the ordinary build's.
CARGO_TARGET_DIR=target/edges WINNOW_GRAMMAR_EDGES=1 cargo test -p winnow-grammar --no-fail-fast "$@"

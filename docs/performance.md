# Checking that a change costs nothing

**The rule:** a change must not make an existing grammar slower. A new
feature may cost something, but only for the grammars that use it - whoever
does not use it pays nothing.

The workflow `Performance` (`.github/workflows/perf.yaml`) checks this on
every pull request with `scripts/compare-perf.sh`, which runs locally as
well:

```sh
scripts/compare-perf.sh                 # origin/main against HEAD
scripts/compare-perf.sh b0a70fb 7ead9c6 # any two commits
```

It needs valgrind and compares committed revisions, not the working tree.

## Instructions, not time

What is compared is the number of instructions executed, counted by
callgrind - not time. A count is **deterministic**: the same binary on the
same input counts the same, run after run, so a difference of one instruction
is a difference in what runs. A time is not: it varies by some percent from
run to run with the machine's load, its frequency and its caches, and in CI
from runner to runner, which buries the differences that matter. The
criterion benchmarks in `benches/` measure time and remain the tool for
absolute speed; a comparison of two revisions uses the counts.

What a count does not see: cache misses, branch mispredictions, the cost of
one instruction against another. A change that keeps the count but moves
memory around can still be slower - that is a case for the benchmarks.

## How the comparison works

1. **Two worktrees, paths of the same length.** BASE and HEAD are checked out
   in `$tmp/a` and `$tmp/b`. A path ends up in the binary (the file names of
   panic locations) and in `argv[0]`, and a longer one costs instructions of
   its own: two builds of the same code in directories of different length
   differ by a few dozen instructions, a difference in no parser.
2. **The same workload.** The profile examples (`examples/*_profile.rs`) and
   `benches/shared/` of HEAD are copied into BASE, so that only the library
   differs. A case whose example does not build on BASE - it uses something
   new - is reported as skipped: there is nothing to compare it with, and a
   new feature is allowed to cost.
3. **Identical binaries first.** If a profile's binary is the same byte for
   byte on both revisions, the compiler generated the same code: nothing can
   have changed, and the case is reported as identical without a second
   count. A change that touches no generated code and no runtime - a
   validator check, a type-level fix, documentation - ends here.
4. **Otherwise counted, in two builds.** Each revision is built twice -
   with the release profile as it is, and with `codegen-units = 1` (*Two
   builds* below) - and each case runs under callgrind in both. A case that
   executes more instructions on HEAD than on BASE by more than
   `PERF_TOLERANCE_PCT` percent (default 0.01) **in both builds** is a
   regression, and the script exits 1. A case slower in one build only is
   reported as a shift in the code layout: a warning, not a failure.

The tolerance is not for noise - there is none - but for what differs between
two different binaries outside the parser: with a different code layout even
the case `none`, which parses nothing, moves by about 0.001 %. A real cost is
far above it: two instructions more per rule call come to +0.1 % in
`cut_profile` and +1 % in `param_profile`.

## A cost that is deliberate

If a pull request makes a case slower on purpose - a new feature that only
its users pay for, but which the profiles exercise - the pull request says
so, with the numbers, and gets the label `perf-accepted`. The job then
passes with a warning; the table stays in the job summary. A case slower in
one build only needs no label.

## Two builds: what a change does, and what it does in practice

The `none` cases show the code layout outside the parser. Inside it a change
can move the hot loop's code as well: other inlining, another register
allocation, an instruction more or less per item that no line of the change
accounts for. Most of that comes from the split into codegen units (*What a
count depends on*, below), so each revision is built twice:

| build | what it shows |
|---|---|
| the release profile as it is (16 codegen units by default) | what a user of the crate gets - the count *in practice* |
| `codegen-units = 1` | what the change itself does: where a function lands no longer moves with an unrelated change, so the difference is mostly the change's own instructions |

The counts of the two builds are not comparable with each other - the one
with a single unit is a different program and here even counts more - only
the differences are. Read together:

* **slower in both** - the change costs something: a regression, or with
  `perf-accepted` a cost that is deliberate;
* **slower in one only** - a shift in the code layout. ADR 28's
  `stream_profile stream` was +0.39 % with 16 units and +0.0006 % with one;
  the reverse happens too (`cut_profile stream_none`, -0.05 % against
  +0.04 %): even a single unit is not free of layout, only freer;
* the real cost ADR 28 first had - a discard per item - was slower in both:
  +14.4 % and +6.0 %.

Where a difference is still unclear, the check by hand remains: remove the
new code from the hot path (or all of it) and count again. If the count does
not fall back to the base - ADR 28 found it *rising* to 53.13 M from
52.73 M, base 52.53 M - the difference is layout, not cost.

## What a count depends on - and is written with

A count is deterministic for one binary, but the binary depends on more than
the source:

* **The compiler.** Another rustc, with another LLVM, inlines and allocates
  registers differently. ADR 28's remaining difference was +0.01 % with
  Rust 1.97.0 and +0.38 % with 1.98.0, for the same source.
* **The split into codegen units.** A release build cuts each crate into
  codegen units (16 by default) and, without LTO, inlines little across
  them. Which function lands in which unit is a heuristic over the whole
  crate, so a change anywhere - even one that adds no code to the hot path -
  can move a function to another unit and change what is inlined into the
  loop. A commit that changed only Markdown moved `cut_profile none` by
  +200 000 instructions (ADR 28).
* **valgrind** and the C library: the count includes what libc's `memcpy`
  and the dynamic loader run, which can differ between versions and
  machines; recorded so that a difference there can be ruled out.

So every number of instructions that is written down - in a pull request,
an ADR, the CHANGELOG - says what it was measured with: the rustc version
(and LLVM), valgrind, and the release profile if it is not Cargo's default.
`scripts/perf-env.sh` prints that line, and `compare-perf.sh`,
`profile-cut.sh` and `profile-stream.sh` put it above their tables (the
first for both of its builds); copy it along with the numbers. Compare with the toolchain `perf.yaml` pins
(`RUSTUP_TOOLCHAIN=1.98.0 scripts/compare-perf.sh`), not the local default:
only those counts are the ones CI checks.

## The profiles

| example | what it exercises |
|---|---|
| `cut_profile` | statements in memory, grouped and from a reader, with and without cuts (ADR 15) |
| `stream_profile` | a fold in memory, under `#[stream]` and with frames (ADR 27 §5); `list`, many short streams with a prefix and a suffix step |
| `slice_profile` | a `#[stream]` rule over an input all in memory (`parse_<rule>_slice`), next to its parse in memory |
| `str_profile` | the same over a `&str` (`parse_<rule>_str`), which skips the second UTF-8 check |
| `param_profile` | rules with value parameters, passed on at nearly every call (ADR 26) |
| `span_profile` | spans bound with `@`, in memory, in a grammar without and with a `#[stream]` rule (whose spans add `origin`) |

`scripts/profile-cut.sh` and `scripts/profile-stream.sh` turn the counts of
one revision into instructions per item, the tables of the ADRs. A new
profile example goes into the list at the top of `scripts/compare-perf.sh`
and into this table.

A case that calls something base does not have goes into a new example, not
into one that is already there: `compare-perf.sh` copies HEAD's examples into
base, and an example that then does not build there has none of its cases
compared - the old ones included. The script warns when that happens.

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
4. **Otherwise counted.** Each case runs under callgrind on both revisions.
   A case that executes more instructions on HEAD than on BASE by more than
   `PERF_TOLERANCE_PCT` percent (default 0.01) is a regression, and the
   script exits 1.

The tolerance is not for noise - there is none - but for what differs between
two different binaries outside the parser: with a different code layout even
the case `none`, which parses nothing, moves by about 0.001 %. A real cost is
far above it: two instructions more per rule call come to +0.1 % in
`cut_profile` and +1 % in `param_profile`.

## A cost that is deliberate

If a pull request makes a case slower on purpose - a new feature that only
its users pay for, but which the profiles exercise - the pull request says
so, with the numbers, and gets the label `perf-accepted`. The job then
passes with a warning; the table stays in the job summary.

## A shift in the code layout

The `none` cases show the code layout outside the parser. Inside it a change
can move the hot loop's code as well: other inlining, another register
allocation, an instruction more or less per item that no line of the change
accounts for. Before calling a difference that, check it: remove the new
code from the hot path (or all of it) and count again. If the count does not
fall back to the base - ADR 28 found it *rising* to 53.13 M from 52.73 M,
base 52.53 M - the difference is layout, not cost; the pull request shows
that check and gets `perf-accepted`. The counts depend on the toolchain:
compare with the one `perf.yaml` pins (`RUSTUP_TOOLCHAIN=1.98.0
scripts/compare-perf.sh`), not the local default.

## The profiles

| example | what it exercises |
|---|---|
| `cut_profile` | statements in memory, grouped and from a reader, with and without cuts (ADR 15) |
| `stream_profile` | a fold in memory, under `#[stream]` and with frames (ADR 27 §5) |
| `param_profile` | rules with value parameters, passed on at nearly every call (ADR 26) |

`scripts/profile-cut.sh` and `scripts/profile-stream.sh` turn the counts of
one revision into instructions per item, the tables of the ADRs. A new
profile example goes into the list at the top of `scripts/compare-perf.sh`
and into this table.

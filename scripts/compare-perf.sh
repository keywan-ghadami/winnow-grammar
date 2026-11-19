#!/usr/bin/env bash
# Compares the instructions two revisions execute in every case of the
# profile examples, counted by callgrind. docs/performance.md says why it is
# done this way. Needs valgrind.
#
#   scripts/compare-perf.sh [BASE] [HEAD]     (defaults: origin/main, HEAD)
#
# Both revisions are compared on the SAME workload: the profile examples and
# `benches/shared/` of HEAD are copied into BASE, so that only the library
# differs. A case whose example does not build on BASE (it uses something
# new) is reported as skipped.
#
# Exits 1 if a case executes more instructions on HEAD than on BASE by more
# than PERF_TOLERANCE_PCT percent (default 0.01). Counts are deterministic -
# the same binary on the same input counts the same, run after run - so the
# tolerance only absorbs what differs between two builds outside the parser.
# The table goes to stdout and, in GitHub Actions, to the job summary.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

base_rev=${1:-origin/main}
head_rev=${2:-HEAD}
tolerance=${PERF_TOLERANCE_PCT:-0.01}

# Every profile example and its cases. A new profile example goes here.
profiles=(
    "cut_profile none stmts_none stmts_cut grouped_none grouped_cut stream_none stream_cut"
    "stream_profile none plain memory stream frames_memory frames_reader"
    "param_profile none yaml"
)

base_sha=$(git rev-parse --verify "$base_rev^{commit}")
head_sha=$(git rev-parse --verify "$head_rev^{commit}")

# `a` and `b`: paths of the same length. A path ends up in the binary (the
# file names of panic locations) and in argv[0], and a longer one costs
# instructions of its own - the counts of two builds in directories of
# different length differ although their code does not.
tmp=$(mktemp -d)
cleanup() {
    git worktree remove --force "$tmp/a" 2>/dev/null || true
    git worktree remove --force "$tmp/b" 2>/dev/null || true
    rm -rf "$tmp"
}
trap cleanup EXIT
git worktree add -q --detach "$tmp/a" "$base_sha"
git worktree add -q --detach "$tmp/b" "$head_sha"

# HEAD's workload in BASE.
rm -rf "$tmp/a/benches/shared"
cp -r "$tmp/b/benches/shared" "$tmp/a/benches/shared"
for spec in "${profiles[@]}"; do
    ex=${spec%% *}
    cp "$tmp/b/examples/$ex.rs" "$tmp/a/examples/$ex.rs"
done

build() { # tree example
    (cd "$tmp/$1" && CARGO_TARGET_DIR="$tmp/$1/target" \
        cargo build -q --release --example "$2" 2>"$tmp/$1-$2.log")
}

refs() { # tree example case
    (cd "$tmp/$1" && valgrind --tool=callgrind --callgrind-out-file=/dev/null \
        "target/release/examples/$2" "$3" 2>&1 >/dev/null |
        awk '/refs/ { gsub(",", "", $NF); print $NF }')
}

report=$(mktemp)
regressions=0
{
    echo "Instructions executed, counted by callgrind: base ${base_sha:0:7}, head ${head_sha:0:7}."
    scripts/perf-env.sh
    echo
    echo "| example | case | base | head | difference |"
    echo "|---|---|---:|---:|---:|"
} >"$report"

for spec in "${profiles[@]}"; do
    read -r ex cases <<<"$spec"
    if ! build b "$ex"; then
        cat "$tmp/b-$ex.log" >&2
        echo "the example $ex does not build on HEAD" >&2
        exit 2
    fi
    if ! build a "$ex"; then
        for c in $cases; do
            echo "| $ex | $c | - | - | skipped: does not build on base |" >>"$report"
        done
        continue
    fi
    identical=false
    if cmp -s "$tmp/a/target/release/examples/$ex" "$tmp/b/target/release/examples/$ex"; then
        identical=true
    fi
    for c in $cases; do
        a=$(refs a "$ex" "$c")
        if $identical; then
            echo "| $ex | $c | $a | $a | 0 (identical binary) |" >>"$report"
            continue
        fi
        b=$(refs b "$ex" "$c")
        diff=$((b - a))
        pct=$(awk -v a="$a" -v d="$diff" 'BEGIN { printf "%+.4f", 100 * d / a }')
        mark=""
        if awk -v p="$pct" -v t="$tolerance" 'BEGIN { exit !(p > t) }'; then
            mark=" **slower**"
            regressions=$((regressions + 1))
        fi
        echo "| $ex | $c | $a | $b | $diff ($pct %)$mark |" >>"$report"
    done
done

if [ "$regressions" -gt 0 ]; then
    printf '\n%s case(s) execute more instructions than on base (tolerance %s %%).\n' \
        "$regressions" "$tolerance" >>"$report"
else
    printf '\nNo case executes more instructions than on base.\n' >>"$report"
fi

cat "$report"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    cat "$report" >>"$GITHUB_STEP_SUMMARY"
fi
rm -f "$report"
[ "$regressions" -eq 0 ]

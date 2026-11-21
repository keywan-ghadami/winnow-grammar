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
# new) is reported as skipped. If that example is already on BASE, none of
# its cases is compared - a warning, since a new case belongs in an example
# of its own then (docs/performance.md).
#
# Each revision is built twice: with the release profile as it is, and with
# codegen-units 1. Exits 1 if a case executes more instructions on HEAD than
# on BASE by more than PERF_TOLERANCE_PCT percent (default 0.01) in BOTH
# builds; a case slower in one build only is a shift in the code layout and
# a warning. Counts are deterministic - the same binary on the same input
# counts the same, run after run - so the tolerance only absorbs what differs
# between two builds outside the parser. The table goes to stdout and, in
# GitHub Actions, to the job summary.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

base_rev=${1:-origin/main}
head_rev=${2:-HEAD}
tolerance=${PERF_TOLERANCE_PCT:-0.01}

# Every profile example and its cases. A new profile example goes here.
profiles=(
    "cut_profile none stmts_none stmts_cut grouped_none grouped_cut stream_none stream_cut"
    "stream_profile none plain memory stream frames_memory frames_reader list"
    "slice_profile none memory slice list doc_none doc_cut"
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

# Each revision is built twice (docs/performance.md, "Two builds"):
#   default - the release profile as it is, what a user of the crate builds;
#   cgu1    - the same with one codegen unit, so that where the compiler
#             puts a function does not move with an unrelated change.
# A case is a regression only if it is slower in both builds; slower in one
# alone is a shift in the code layout and reported as a warning.
variants=(default cgu1)

build() { # tree example variant
    local cgu=()
    if [ "$3" = cgu1 ]; then
        cgu=(CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1)
    fi
    (cd "$tmp/$1" && env -u CARGO_PROFILE_RELEASE_CODEGEN_UNITS "${cgu[@]}" \
        CARGO_TARGET_DIR="$tmp/$1/target-$3" \
        cargo build -q --release --example "$2" 2>"$tmp/$1-$2-$3.log")
}

refs() { # tree example case variant
    (cd "$tmp/$1" && valgrind --tool=callgrind --callgrind-out-file=/dev/null \
        "target-$4/release/examples/$2" "$3" 2>&1 >/dev/null |
        awk '/refs/ { gsub(",", "", $NF); print $NF }')
}

# One build's comparison of one case: "base head difference" and whether it
# is slower than the tolerance allows.
compare() { # example case variant identical
    local a b diff pct
    a=$(refs a "$1" "$2" "$3")
    if [ "$4" = true ]; then
        echo "$a $a 0 (identical binary)"
        return
    fi
    b=$(refs b "$1" "$2" "$3")
    diff=$((b - a))
    pct=$(awk -v a="$a" -v d="$diff" 'BEGIN { printf "%+.4f", 100 * d / a }')
    echo "$a $b $diff ($pct %)"
}

slower() { # "difference (pct %)"
    local pct
    case $1 in *" %)") ;; *) return 1 ;; esac
    pct=$(sed -E 's/.*\(([-+0-9.]+) %\).*/\1/' <<<"$1")
    awk -v p="$pct" -v t="$tolerance" 'BEGIN { exit !(p > t) }' 2>/dev/null
}

report=$(mktemp)
regressions=0
shifts=0
unchecked=()
{
    echo "Instructions executed, counted by callgrind: base ${base_sha:0:7}, head ${head_sha:0:7}."
    scripts/perf-env.sh
    echo "Each revision is built twice: with that profile, and with codegen-units 1."
    echo
    echo "| example | case | base | head | difference | base, 1 CGU | head, 1 CGU | difference, 1 CGU | |"
    echo "|---|---|---:|---:|---:|---:|---:|---:|---|"
} >"$report"

for spec in "${profiles[@]}"; do
    read -r ex cases <<<"$spec"
    skipped=false
    identical=()
    for v in "${variants[@]}"; do
        if ! build b "$ex" "$v"; then
            cat "$tmp/b-$ex-$v.log" >&2
            echo "the example $ex does not build on HEAD" >&2
            exit 2
        fi
        if ! build a "$ex" "$v"; then
            skipped=true
            break
        fi
        if cmp -s "$tmp/a/target-$v/release/examples/$ex" "$tmp/b/target-$v/release/examples/$ex"; then
            identical+=(true)
        else
            identical+=(false)
        fi
    done
    if $skipped; then
        why="skipped: does not build on base"
        if git cat-file -e "$base_sha:examples/$ex.rs" 2>/dev/null; then
            # The example is on BASE, and HEAD's no longer builds there:
            # its existing cases go unchecked.
            why="**unchecked**: on base, but HEAD's version does not build there"
            unchecked+=("$ex")
        fi
        for c in $cases; do
            echo "| $ex | $c | - | - | - | - | - | - | $why |" >>"$report"
        done
        continue
    fi
    for c in $cases; do
        read -r a1 b1 d1 <<<"$(compare "$ex" "$c" default "${identical[0]}")"
        read -r a2 b2 d2 <<<"$(compare "$ex" "$c" cgu1 "${identical[1]}")"
        verdict=""
        s1=false s2=false
        slower "$d1" && s1=true
        slower "$d2" && s2=true
        if $s1 && $s2; then
            verdict="**slower**"
            regressions=$((regressions + 1))
        elif $s1 || $s2; then
            verdict="layout: slower in one build only"
            shifts=$((shifts + 1))
        fi
        echo "| $ex | $c | $a1 | $b1 | $d1 | $a2 | $b2 | $d2 | $verdict |" >>"$report"
    done
done

echo >>"$report"
if [ "$regressions" -gt 0 ]; then
    printf '%s case(s) execute more instructions than on base in both builds (tolerance %s %%).\n' \
        "$regressions" "$tolerance" >>"$report"
else
    printf 'No case executes more instructions than on base in both builds.\n' >>"$report"
fi
if [ "${#unchecked[@]}" -gt 0 ]; then
    printf 'Not compared at all: %s. Each is on base, but uses something new on HEAD; a new case goes into an example of its own (docs/performance.md).\n' \
        "${unchecked[*]}" >>"$report"
fi
if [ "$shifts" -gt 0 ]; then
    printf '%s case(s) are slower in one build only: a shift in the code layout, not a cost (docs/performance.md).\n' \
        "$shifts" >>"$report"
fi

cat "$report"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    cat "$report" >>"$GITHUB_STEP_SUMMARY"
fi
if [ "$shifts" -gt 0 ] && [ -n "${GITHUB_ACTIONS:-}" ]; then
    echo "::warning::$shifts case(s) slower in one build only - a shift in the code layout; see the job summary."
fi
if [ "${#unchecked[@]}" -gt 0 ] && [ -n "${GITHUB_ACTIONS:-}" ]; then
    echo "::warning::Not compared, on base but not buildable there with HEAD's version: ${unchecked[*]} - put a new case into an example of its own."
fi
rm -f "$report"
[ "$regressions" -eq 0 ]

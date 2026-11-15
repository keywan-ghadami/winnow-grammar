#!/usr/bin/env bash
# Fails if a tracked `.rs` file is compiled by no target.
#
# Rust compiles only what the module tree reaches: a file whose `mod` line was
# deleted, or that was never given one, is silently ignored - its tests do not
# run, its code is not checked, and it rots. The compiler's dep-info says
# exactly which files it read, so this compares that with `git ls-files`.
#
# Run it after `cargo check` / `cargo clippy --workspace --all-targets`, which
# write the dep-info (`target/debug/deps/*.d`, examples in
# `target/debug/examples/*.d`). The files trybuild compiles on
# its own (`tests/ui/*.rs`) are taken from `tests/ui_tests.rs`, so a UI case
# that is not registered there is reported as well.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

debug="${CARGO_TARGET_DIR:-target}/debug"
shopt -s nullglob
dep_info=("$debug"/deps/*.d "$debug"/examples/*.d)
if [ ${#dep_info[@]} -eq 0 ]; then
    echo "no dep-info in $debug - run cargo check --workspace --all-targets first" >&2
    exit 2
fi

compiled=$(
    {
        # dep-info: `out: src1 src2 …`, absolute paths, spaces escaped.
        cat "${dep_info[@]}" | tr ' ' '\n' | grep '\.rs$' | sed "s|^$PWD/||"
        # trybuild cases.
        grep -o '"tests/ui/[^"]*\.rs"' tests/ui_tests.rs | tr -d '"'
    } | sort -u
)

orphans=$(comm -23 <(git ls-files '*.rs' | sort) <(echo "$compiled"))

if [ -n "$orphans" ]; then
    echo "These files are compiled by no target - add a \`mod\` for them or delete them:" >&2
    echo "$orphans" | sed 's/^/  /' >&2
    exit 1
fi
echo "every tracked .rs file is compiled"

#!/usr/bin/env bash
# Prints what a count of instructions depends on besides the code, as one
# line for the tables of compare-perf.sh, profile-cut.sh and
# profile-stream.sh: the compiler (rustc and its LLVM), valgrind, and the
# release profile the examples are built with. docs/performance.md says why:
# the same source counts differently with another compiler, and with another
# split into codegen units.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

rustc_line=$(rustc -vV)
release=$(awk '/^release:/ { print $2 }' <<<"$rustc_line")
commit=$(awk '/^commit-hash:/ { print substr($2, 1, 9) }' <<<"$rustc_line")
llvm=$(awk '/^LLVM version:/ { print $3 }' <<<"$rustc_line")
valgrind_v=$(valgrind --version 2>/dev/null || echo "valgrind missing")

# The release profile: an environment override, else Cargo.toml's
# [profile.release], else Cargo's default.
setting() { # KEY ENV DEFAULT
    local v=${!2:-}
    if [ -z "$v" ]; then
        v=$(awk -v k="$1" '
            /^\[/ { in_release = ($0 == "[profile.release]") }
            in_release && $1 == k { sub(/^[^=]*=[ \t]*/, ""); print; exit }
        ' Cargo.toml)
    fi
    echo "${v:-$3}"
}
cgu=$(setting codegen-units CARGO_PROFILE_RELEASE_CODEGEN_UNITS "16 (default)")
lto=$(setting lto CARGO_PROFILE_RELEASE_LTO "false (default)")

echo "Measured with rustc $release ($commit, LLVM $llvm), $valgrind_v; release profile: codegen-units $cgu, lto $lto."

#!/bin/sh
# Instructions per statement of every case of `examples/cut_profile.rs`,
# counted by callgrind, the cost of building the input (`none`) subtracted -
# the table in ADR 15. Needs valgrind.
set -e
cargo build --release --example cut_profile
bin=target/release/examples/cut_profile
out=$(mktemp)
refs() {
    valgrind --tool=callgrind --callgrind-out-file="$out" "$bin" "$1" 2>&1 >/dev/null |
        awk '/refs/ { gsub(",", "", $NF); print $NF }'
}
base=$(refs none)
for case in stmts_none stmts_cut grouped_none grouped_cut stream_none stream_cut; do
    printf '%-14s %s instructions/statement\n' "$case" \
        "$(( ($(refs "$case") - base) / 200000 ))"
done
rm -f "$out"

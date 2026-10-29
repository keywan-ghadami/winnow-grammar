#!/bin/sh
# Instructions per row of every case of `examples/stream_profile.rs`, counted
# by callgrind, the cost of building the input (`none`) subtracted - the
# second table of ADR 27 §5. Needs valgrind.
set -e
cargo build --release --example stream_profile
bin=target/release/examples/stream_profile
out=$(mktemp)
refs() {
    valgrind --tool=callgrind --callgrind-out-file="$out" "$bin" "$1" 2>&1 >/dev/null |
        awk '/refs/ { gsub(",", "", $NF); print $NF }'
}
base=$(refs none)
for case in plain memory stream frames_memory frames_reader; do
    printf '%-14s %s instructions/row\n' "$case" \
        "$(( ($(refs "$case") - base) / 200000 ))"
done
rm -f "$out"

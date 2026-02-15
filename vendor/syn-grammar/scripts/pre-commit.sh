#!/bin/sh
# Format all code
cargo fmt

# Re-add all staged rust files to include formatting changes
# If there are staged rust files, add them again to capture formatting changes
FILES=$(git diff --name-only --cached | grep '\.rs$')
if [ -n "$FILES" ]; then
    echo "$FILES" | xargs git add
fi

# Ensure no clippy warnings are present across all workspace members and targets
cargo clippy --workspace --all-targets -- -D warnings

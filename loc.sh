#!/bin/bash
# List all non-test Rust source files by line count (descending)
# Excludes: target/, tests.rs files, and files in tests/ directories

find . -name "*.rs" \
    -not -path "./target/*" \
    -not -name "tests.rs" \
    -not -path "*/tests/*" \
    -exec wc -l {} + | \
    grep -v ' total$' | \
    sort -rn

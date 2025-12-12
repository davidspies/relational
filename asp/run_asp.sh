#!/bin/bash
# Run an ASP program through gringo and the asp solver, then verify
# Usage: ./run_asp.sh [-n LIMIT] program.lp [more_files.lp ...]
#    or: echo "a :- not b." | ./run_asp.sh [-n LIMIT]
#
# Options:
#   -n LIMIT  Number of solutions to find (default: 1, use 0 for all)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ASP_BIN="${SCRIPT_DIR}/../target/release/asp"
VERIFY="${SCRIPT_DIR}/verify_solution.py"

# Parse -n option
LIMIT=1
if [ "$1" = "-n" ]; then
    LIMIT="$2"
    shift 2
fi

# Build before running
cargo build --release -p asp --quiet

if [ -n "$1" ]; then
    # File arguments provided - pass all to gringo
    OUTPUT=$(gringo --output=smodels "$@" | "$ASP_BIN" "$LIMIT" 2>&1)
    echo "$OUTPUT"
    echo ""
    echo "$OUTPUT" | python3 "$VERIFY" "$@"
else
    # Read from stdin - can't verify without files
    gringo --output=smodels | "$ASP_BIN" "$LIMIT"
fi

#!/bin/bash
# Run an ASP program through gringo and the asp solver
# Usage: ./run_asp.sh program.lp
#    or: echo "a :- not b." | ./run_asp.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ASP_BIN="${SCRIPT_DIR}/../target/release/asp"

# Build before running
cargo build --release -p asp --quiet

if [ -n "$1" ]; then
    # File argument provided
    gringo --output=smodels "$1" | "$ASP_BIN"
else
    # Read from stdin
    gringo --output=smodels | "$ASP_BIN"
fi

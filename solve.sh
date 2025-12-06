#!/bin/bash
# Fuzzy-match a CNF benchmark and run the solver

set -e

if ! command -v fzf &> /dev/null; then
    echo "fzf is required: sudo apt install fzf"
    exit 1
fi

if [ ! -d cnf_benchmarks ]; then
    echo "No cnf_benchmarks/ directory. Run ./download_cnf_benchmarks.sh first."
    exit 1
fi

file=$(find cnf_benchmarks -name "*.cnf" | fzf --query="$1" --select-1 --exit-0)

if [ -z "$file" ]; then
    echo "No file selected"
    exit 1
fi

# Derive output names from input file
basename=$(basename "$file" .cnf)
mkdir -p solve_output
svg_file="solve_output/${basename}.svg"
proof_file="solve_output/${basename}.drat"

echo "Running: $file"
echo "  SVG:   $svg_file"
echo "  Proof: $proof_file"
echo "---"

output=$(cargo run --release -p cdcl -- "$file" --svg "$svg_file" --proof "$proof_file")
echo "$output"

if echo "$output" | grep -q "^s UNSATISFIABLE"; then
    echo "---"
    echo "Verifying proof with drat-trim..."
    make -s bin/drat-trim
    bin/drat-trim "$file" "$proof_file"
elif echo "$output" | grep -q "^s SATISFIABLE"; then
    echo "---"
    echo "Verifying solution..."
    echo "$output" | ./verify_sat.py "$file"
    echo "Verifying proof derivation with drat-trim..."
    make -s bin/drat-trim
    bin/drat-trim "$file" "$proof_file" -f
fi

#!/bin/bash
# Download CNF benchmark files for SAT solver testing
#
# Sources:
#   - SATLIB: https://www.cs.ubc.ca/~hoos/SATLIB/benchm.html
#   - SAT Competition archives on Zenodo

set -e

DEST_DIR="cnf_benchmarks"
SATLIB_BASE="https://www.cs.ubc.ca/~hoos/SATLIB"

mkdir -p "$DEST_DIR"
cd "$DEST_DIR"

echo "=== Downloading CNF benchmarks to $DEST_DIR ==="

# SATLIB Uniform Random-3-SAT (good for testing, various difficulty levels)
echo ""
echo "--- SATLIB: Uniform Random-3-SAT ---"

# uf20-91: 20 vars, 91 clauses, 1000 satisfiable instances (easy)
if [ ! -d "uf20-91" ]; then
    echo "Downloading uf20-91 (20 vars, 91 clauses, 1000 SAT instances)..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/RND3SAT/uf20-91.tar.gz"
    tar xzf uf20-91.tar.gz && rm uf20-91.tar.gz
fi

# uf50-218: 50 vars, 218 clauses, 1000 satisfiable instances (medium)
if [ ! -d "uf50-218" ]; then
    echo "Downloading uf50-218 (50 vars, 218 clauses, 1000 SAT instances)..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/RND3SAT/uf50-218.tar.gz"
    tar xzf uf50-218.tar.gz && rm uf50-218.tar.gz
fi

# uuf50-218: 50 vars, 218 clauses, 1000 unsatisfiable instances
if [ ! -d "uuf50-218" ]; then
    echo "Downloading uuf50-218 (50 vars, 218 clauses, 1000 UNSAT instances)..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/RND3SAT/uuf50-218.tar.gz"
    tar xzf uuf50-218.tar.gz && rm uuf50-218.tar.gz
fi

# uf75-325: 75 vars, 325 clauses (harder)
if [ ! -d "uf75-325" ]; then
    echo "Downloading uf75-325 (75 vars, 325 clauses, 100 SAT instances)..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/RND3SAT/uf75-325.tar.gz"
    tar xzf uf75-325.tar.gz && rm uf75-325.tar.gz
fi

# uf100-430: 100 vars, 430 clauses (challenging)
if [ ! -d "uf100-430" ]; then
    echo "Downloading uf100-430 (100 vars, 430 clauses, 1000 SAT instances)..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/RND3SAT/uf100-430.tar.gz"
    tar xzf uf100-430.tar.gz && rm uf100-430.tar.gz
fi

# Graph Coloring Problems (structured, real-world-ish)
echo ""
echo "--- SATLIB: Graph Coloring ---"

if [ ! -d "flat30-60" ]; then
    echo "Downloading flat30-60 (graph coloring, smaller)..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/GCP/flat30-60.tar.gz"
    tar xzf flat30-60.tar.gz && rm flat30-60.tar.gz
fi

if [ ! -d "flat50-115" ]; then
    echo "Downloading flat50-115 (graph coloring, medium)..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/GCP/flat50-115.tar.gz"
    tar xzf flat50-115.tar.gz && rm flat50-115.tar.gz
fi

# Planning problems (Blocksworld)
echo ""
echo "--- SATLIB: Planning (Blocksworld) ---"

if [ ! -d "blocksworld" ]; then
    echo "Downloading blocksworld planning problems..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/PLANNING/BlocksWorld/blocksworld.tar.gz"
    tar xzf blocksworld.tar.gz && rm blocksworld.tar.gz
fi

# All-Interval Series (mathematical)
echo ""
echo "--- SATLIB: All-Interval Series ---"

if [ ! -d "ais" ]; then
    echo "Downloading all-interval series..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/AIS/ais.tar.gz"
    tar xzf ais.tar.gz && rm ais.tar.gz
fi

# Dubois instances (crafted hard instances)
echo ""
echo "--- SATLIB: Dubois (crafted UNSAT) ---"

if [ ! -d "dubois" ]; then
    echo "Downloading dubois instances (crafted UNSAT)..."
    curl -LO "$SATLIB_BASE/Benchmarks/SAT/DIMACS/DUBOIS/dubois.tar.gz"
    tar xzf dubois.tar.gz && rm dubois.tar.gz
fi

echo ""
echo "=== Download complete! ==="
echo ""
echo "Benchmark summary:"
echo "  uf20-91/     - Easy: 20 vars, SAT instances"
echo "  uf50-218/    - Medium: 50 vars, SAT instances"
echo "  uuf50-218/   - Medium: 50 vars, UNSAT instances"
echo "  uf75-325/    - Harder: 75 vars, SAT instances"
echo "  uf100-430/   - Hard: 100 vars, SAT instances"
echo "  flat30-60/   - Graph coloring (structured)"
echo "  flat50-115/  - Graph coloring (structured)"
echo "  blocksworld/ - Planning problems"
echo "  ais/         - All-interval series"
echo "  dubois/      - Crafted UNSAT instances"
echo ""
echo "Example usage:"
echo "  cargo run --release -p cdcl -- cnf_benchmarks/uf20-91/uf20-01.cnf"

#!/bin/bash
# Download ASP benchmark files for ASP solver testing
#
# Sources:
#   - Potassco ASP Planning Benchmarks: https://github.com/potassco/asp-planning-benchmarks
#   - ASP Competition 2013: https://www.mat.unical.it/aspcomp2013/OfficialProblemSuite
#
# Note: These are .lp files. Use gringo to convert to smodels format:
#   gringo --output=smodels problem.lp | ./target/release/asp

set -e

DEST_DIR="asp_benchmarks"

mkdir -p "$DEST_DIR"
cd "$DEST_DIR"

echo "=== Downloading ASP benchmarks to $DEST_DIR ==="

# Potassco ASP Planning Benchmarks (from GitHub)
echo ""
echo "--- Potassco ASP Planning Benchmarks ---"

if [ ! -d "asp-planning-benchmarks" ]; then
    echo "Cloning potassco/asp-planning-benchmarks..."
    git clone --depth 1 https://github.com/potassco/asp-planning-benchmarks.git
else
    echo "asp-planning-benchmarks already exists, skipping..."
fi

# ASP Competition 2013 - Try to get the benchmark suite
echo ""
echo "--- ASP Competition 2013 Benchmarks ---"

ASPCOMP_URL="https://www.mat.unical.it/ianni/aspcomp2013"

if [ ! -d "aspcomp2013" ]; then
    mkdir -p aspcomp2013
    cd aspcomp2013

    echo "Attempting to download ASP Competition 2013 benchmarks..."
    if curl -fLO "${ASPCOMP_URL}/aspcomp2013-encodings-samples-checkers.tar.gz" 2>/dev/null; then
        echo "Extracting full benchmark suite..."
        tar xzf aspcomp2013-encodings-samples-checkers.tar.gz && rm aspcomp2013-encodings-samples-checkers.tar.gz
    else
        echo "(ASP Competition archive not available - server may be down)"
    fi

    cd ..
else
    echo "aspcomp2013 already exists, skipping..."
fi

echo ""
echo "=== Download complete! ==="
echo ""
echo "Benchmark summary:"
echo "  asp-planning-benchmarks/  - Potassco planning (HanoiTower, Sokoban, Labyrinth, etc.)"
echo "  aspcomp2013/              - ASP Competition 2013 (if available)"
echo ""
echo "Usage (requires gringo from Potassco):"
echo "  gringo --output=smodels asp_benchmarks/classic/queens_8.lp | cargo run --release -p asp"

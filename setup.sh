#!/bin/bash
set -e

echo "Setting up relational project..."

# Initialize and update submodules
echo "Initializing git submodules..."
git submodule update --init --recursive

# Create Python venv and install VeriPB
echo "Creating Python virtual environment..."
python3 -m venv .venv

echo "Installing VeriPB..."
.venv/bin/pip install ./vendor/veripb

# Build Rust project
echo "Building Rust project..."
cargo build --release

echo ""
echo "Setup complete!"
echo ""
echo "To use veripb, activate the venv:"
echo "  source .venv/bin/activate"
echo "  veripb --help"

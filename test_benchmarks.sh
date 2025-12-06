#!/bin/bash
# Test all CNF benchmarks, reporting timeouts and verification failures

set -e

TIMEOUT_SECS=5
SOLVER="target/release/cdcl"
SKIP_DRAT=${SKIP_DRAT:-0}  # Set SKIP_DRAT=1 to skip DRAT verification

# Build solver
cargo build --release -p cdcl 2>/dev/null

# Build drat-trim
make -s bin/drat-trim

# Create output directory
mkdir -p solve_output

failed=()
timedout=()

print_summary() {
    echo ""
    echo "====== SUMMARY ======"
    echo "Timeouts (${#timedout[@]}):"
    for f in "${timedout[@]}"; do
        echo "  $f"
    done
    echo ""
    echo "Failures (${#failed[@]}):"
    for f in "${failed[@]}"; do
        echo "  $f"
    done
}

trap 'echo ""; echo "Interrupted!"; print_summary; exit 130' INT

for file in $(find cnf_benchmarks -name "*.cnf" | sort); do
    basename=$(basename "$file" .cnf)
    proof_file="solve_output/${basename}.drat"
    svg_file="solve_output/${basename}.svg"

    # Run solver with timeout
    set +e
    output=$(timeout --signal=INT ${TIMEOUT_SECS} "$SOLVER" "$file" --proof "$proof_file" --svg "$svg_file" 2>&1)
    exit_code=$?
    set -e

    if [ $exit_code -eq 124 ]; then
        echo "TIMEOUT: $file"
        timedout+=("$file")
        continue
    elif [ $exit_code -ne 0 ]; then
        echo "CRASH: $file (exit code $exit_code)"
        failed+=("$file (crash: $exit_code)")
        continue
    fi

    # Check result type
    case "$output" in
        *"s UNSATISFIABLE"*)
            if [ "$SKIP_DRAT" = "1" ]; then
                echo "OK: $file (UNSAT, not verified)"
            else
                set +e
                verify_output=$(bin/drat-trim "$file" "$proof_file" 2>&1)
                set -e
                if [[ "$verify_output" == *"s VERIFIED"* ]]; then
                    echo "OK: $file (UNSAT)"
                else
                    echo "FAIL: $file (DRAT proof invalid)"
                    failed+=("$file (DRAT invalid)")
                fi
            fi
            ;;
        *"s SATISFIABLE"*)
            set +e
            verify_output=$(echo "$output" | ./verify_sat.py "$file" 2>&1)
            verify_code=$?
            set -e
            if [ $verify_code -ne 0 ]; then
                echo "FAIL: $file (SAT solution invalid)"
                failed+=("$file (SAT invalid)")
                continue
            fi
            if [ "$SKIP_DRAT" = "1" ]; then
                echo "OK: $file (SAT, proof not verified)"
            else
                set +e
                verify_output=$(bin/drat-trim "$file" "$proof_file" -f 2>&1)
                set -e
                if [[ "$verify_output" == *"s DERIVATION"* ]]; then
                    echo "OK: $file (SAT)"
                else
                    echo "FAIL: $file (DRAT derivation invalid)"
                    failed+=("$file (DRAT invalid)")
                fi
            fi
            ;;
        *)
            echo "FAIL: $file (no result)"
            failed+=("$file (no result)")
            ;;
    esac
done

print_summary

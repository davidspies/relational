#!/usr/bin/env python3
"""Run ASP programs through gringo and our solver, with optional verification."""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path


def build_solver(script_dir: Path) -> Path:
    """Build the ASP solver and return path to binary."""
    workspace = script_dir.parent
    subprocess.run(
        ["cargo", "build", "--release", "-p", "asp", "--quiet"],
        cwd=workspace,
        check=True,
    )
    return workspace / "target" / "release" / "asp"


def get_all_atoms_from_smodels(smodels: str) -> set[str]:
    """Extract all atom names from smodels format."""
    atoms = set()
    in_symbols = False
    for line in smodels.split("\n"):
        line = line.strip()
        if line == "0" and not in_symbols:
            in_symbols = True
            continue
        if in_symbols:
            if line == "0":
                break
            parts = line.split(None, 1)
            if len(parts) == 2:
                atoms.add(parts[1])
    return atoms


def generate_constraints(atoms_true: list[str], atoms_false: list[str]) -> str:
    """Generate ASP constraints to fix the model."""
    lines = []
    for atom in atoms_true:
        lines.append(f":- not {atom}.")
    for atom in atoms_false:
        lines.append(f":- {atom}.")
    return "\n".join(lines)


def verify_with_clasp(
    original_program: str, atoms_true: list[str], all_atoms: set[str]
) -> bool:
    """Verify the solution using clasp."""
    atoms_false = [a for a in all_atoms if a not in atoms_true]
    constraints = generate_constraints(atoms_true, atoms_false)
    combined = original_program + "\n" + constraints

    try:
        gringo = subprocess.run(
            ["gringo"],
            input=combined,
            capture_output=True,
            text=True,
            timeout=30,
        )
        if gringo.returncode != 0:
            print(f"gringo failed: {gringo.stderr}", file=sys.stderr)
            return False

        clasp = subprocess.run(
            ["clasp", "1"],
            input=gringo.stdout,
            capture_output=True,
            text=True,
            timeout=30,
        )
        return "SATISFIABLE" in clasp.stdout
    except subprocess.TimeoutExpired:
        print("Timeout running clasp", file=sys.stderr)
        return False
    except FileNotFoundError as e:
        print(f"Command not found: {e}", file=sys.stderr)
        return False


def verify_unsat(original_program: str) -> bool:
    """Verify UNSAT result with clasp."""
    print("Our solver returned UNSATISFIABLE, verifying with clasp...")
    try:
        gringo = subprocess.run(
            ["gringo"],
            input=original_program,
            capture_output=True,
            text=True,
            timeout=60,
        )
        clasp = subprocess.run(
            ["clasp", "1"],
            input=gringo.stdout,
            capture_output=True,
            text=True,
            timeout=60,
        )
        if "UNSATISFIABLE" in clasp.stdout:
            print("VERIFIED: clasp also returns UNSATISFIABLE")
            return True
        else:
            print("MISMATCH: clasp found a solution but we returned UNSAT")
            return False
    except Exception as e:
        print(f"Error running clasp: {e}", file=sys.stderr)
        return False


def verify_answer_sets(
    original_program: str, answer_sets: list[list[str]]
) -> bool:
    """Verify all answer sets are valid."""
    try:
        gringo = subprocess.run(
            ["gringo", "--output=smodels"],
            input=original_program,
            capture_output=True,
            text=True,
            timeout=60,
        )
        all_atoms = get_all_atoms_from_smodels(gringo.stdout)
    except Exception as e:
        print(f"Error getting atoms: {e}", file=sys.stderr)
        return False

    print(f"Found {len(answer_sets)} answer set(s)")

    all_valid = True
    for i, atoms_true in enumerate(answer_sets):
        print(f"\nVerifying answer set {i+1}: {len(atoms_true)} atoms")
        if verify_with_clasp(original_program, atoms_true, all_atoms):
            print(f"  Answer set {i+1}: VALID")
        else:
            print(f"  Answer set {i+1}: INVALID")
            all_valid = False

    if all_valid:
        print("\nVERIFIED: All solutions are valid answer sets")
    else:
        print("\nINVALID: Some solutions are not valid answer sets")
    return all_valid


def run_solver(
    asp_bin: Path,
    limit: int,
    program_files: list[str],
    verify: bool,
) -> int:
    """Run gringo | asp solver, streaming output. Returns exit code."""
    # Read program files
    original_program = ""
    for f in program_files:
        with open(f) as fp:
            original_program += fp.read() + "\n"

    # Run gringo
    gringo = subprocess.Popen(
        ["gringo", "--output=smodels"] + program_files,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    # Run our solver (inherit stderr for real-time stats)
    solver = subprocess.Popen(
        [str(asp_bin), str(limit)],
        stdin=gringo.stdout,
        stdout=subprocess.PIPE,
        stderr=None,  # Inherit stderr
        text=True,
    )
    gringo.stdout.close()

    # Stream output and collect answer sets
    answer_sets: list[list[str]] = []
    is_sat = None

    while True:
        line = solver.stdout.readline()
        if not line:
            break

        sys.stdout.write(line)
        sys.stdout.flush()

        if "UNSATISFIABLE" in line:
            is_sat = False
        elif "SATISFIABLE" in line:
            is_sat = True

        if line.startswith("Answer"):
            match = re.match(r"Answer \d+: (.*)", line)
            if match:
                atoms = match.group(1).split() if match.group(1).strip() else []
                answer_sets.append(atoms)

    solver.wait()
    gringo.wait()

    if not verify:
        return 0

    # Verification
    if is_sat is None:
        print("ERROR: Could not determine satisfiability", file=sys.stderr)
        return 1

    if not is_sat:
        return 0 if verify_unsat(original_program) else 1

    return 0 if verify_answer_sets(original_program, answer_sets) else 1


def main():
    parser = argparse.ArgumentParser(
        description="Run ASP programs through gringo and our solver"
    )
    parser.add_argument(
        "-n",
        type=int,
        default=1,
        help="Number of solutions to find (0 for all, default: 1)",
    )
    parser.add_argument(
        "--no-verify",
        action="store_true",
        help="Skip verification with clasp",
    )
    parser.add_argument(
        "files",
        nargs="+",
        help="ASP program files",
    )
    args = parser.parse_args()

    script_dir = Path(__file__).parent.resolve()

    try:
        asp_bin = build_solver(script_dir)
    except subprocess.CalledProcessError:
        print("Failed to build solver", file=sys.stderr)
        sys.exit(1)

    exit_code = run_solver(
        asp_bin,
        args.n,
        args.files,
        verify=not args.no_verify,
    )
    sys.exit(exit_code)


if __name__ == "__main__":
    main()

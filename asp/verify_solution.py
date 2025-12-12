#!/usr/bin/env python3
"""Verify ASP solution by constraining to exact model and checking with clasp."""

import subprocess
import sys
import re


def parse_our_output(output: str) -> tuple[list[list[str]], bool]:
    """Parse our solver's output. Returns (list of answer sets, is_sat)."""
    if "UNSATISFIABLE" in output:
        return [], False
    if "SATISFIABLE" not in output:
        return [], False

    answer_sets = []
    for line in output.split("\n"):
        if line.startswith("Answer"):
            # Parse all atoms from the answer line
            # Format: "Answer 1: atom1 atom2 atom3..."
            match = re.match(r"Answer \d+: (.*)", line)
            if match:
                atoms = match.group(1).split() if match.group(1).strip() else []
                answer_sets.append(atoms)
    return answer_sets, True


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
            # Format: "atom_id name"
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


def verify_with_clasp(original_program: str, atoms_true: list[str], all_atoms: set[str]) -> bool:
    """Verify the solution using clasp."""
    atoms_false = [a for a in all_atoms if a not in atoms_true]
    constraints = generate_constraints(atoms_true, atoms_false)

    # Combine original program with constraints
    combined = original_program + "\n" + constraints

    # Run through gringo and clasp
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


def main():
    if len(sys.argv) < 2:
        print("Usage: verify_solution.py <program.lp> [more_files.lp ...]", file=sys.stderr)
        print("  Reads our solver's output from stdin", file=sys.stderr)
        sys.exit(1)

    # Concatenate all program files
    original_program = ""
    for program_file in sys.argv[1:]:
        with open(program_file) as f:
            original_program += f.read() + "\n"

    our_output = sys.stdin.read()
    answer_sets, is_sat = parse_our_output(our_output)

    if not is_sat:
        # For UNSAT, verify with clasp directly
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
                sys.exit(0)
            else:
                print("MISMATCH: clasp found a solution but we returned UNSAT")
                sys.exit(1)
        except Exception as e:
            print(f"Error running clasp: {e}", file=sys.stderr)
            sys.exit(1)

    # Get all atoms from gringo output
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
        sys.exit(1)

    print(f"Found {len(answer_sets)} answer set(s)")
    print(f"All visible atoms ({len(all_atoms)}): {sorted(all_atoms)[:10]}{'...' if len(all_atoms) > 10 else ''}")

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
        sys.exit(0)
    else:
        print("\nINVALID: Some solutions are not valid answer sets")
        sys.exit(1)


if __name__ == "__main__":
    main()

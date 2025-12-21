#!/usr/bin/env python3
"""
Test if the candidate model from a specific constraint is a valid stable model.

Takes the candidate model from our solver (which triggered a loop constraint),
adds integrity constraints to force each atom to its value, and runs clasp
to see if it's SAT or UNSAT.

If SAT: the candidate model IS a valid stable model, so our loop constraint is wrong.
If UNSAT: the candidate model is NOT a valid stable model, we correctly rejected it.
"""

import subprocess
import sys
import os
import re

def get_gringo_output(lp_file, add_names=False):
    """Get smodels format from gringo, optionally adding names for all atoms."""
    result = subprocess.run(
        ['gringo', '--output=smodels', lp_file],
        capture_output=True, text=True
    )
    if result.returncode != 0:
        print(f"gringo error: {result.stderr}")
        sys.exit(1)

    smodels = result.stdout

    if add_names:
        # Add names for all atoms using name_all_atoms.py
        script_dir = os.path.dirname(__file__)
        name_script = os.path.join(script_dir, 'name_all_atoms.py')
        result = subprocess.run(
            ['python3', name_script],
            input=smodels,
            capture_output=True, text=True
        )
        smodels = result.stdout

    return smodels

def parse_smodels_max_atom(smodels):
    """Find the maximum atom ID in the smodels output."""
    max_atom = 0
    for line in smodels.split('\n'):
        parts = line.split()
        if not parts:
            continue
        # Rule lines start with rule type, then have atom IDs
        for part in parts[1:]:
            try:
                atom_id = int(part)
                if atom_id > max_atom:
                    max_atom = atom_id
            except ValueError:
                continue
    return max_atom

def get_constraint_candidate_model(lp_file, constraint_idx):
    """Run our solver and extract the candidate model for the given constraint."""
    smodels = get_gringo_output(lp_file)

    env = os.environ.copy()
    env['ASP_DEBUG'] = '1'
    env['ASP_DUMP_CONSTRAINT'] = str(constraint_idx)

    result = subprocess.run(
        ['./target/release/asp', '0'],
        input=smodels,
        capture_output=True, text=True,
        env=env
    )

    # Parse the candidate model from stderr
    # Format: "c CANDIDATE_MODEL: 011010101..." (one char per atom, no spaces)
    for line in result.stderr.split('\n'):
        if line.startswith('c CANDIDATE_MODEL:'):
            bits = line.split(':')[1].strip()
            return [c == '1' for c in bits]

    return None

def add_integrity_constraints(smodels, candidate_model):
    """
    Add integrity constraints to force each atom to its value in the model.

    In smodels format:
    - Rule type 1 is a basic rule: 1 head num_lits num_neg lit1 lit2 ...
    - To force atom A to be true: add a constraint (headless rule) with body = {not A}
      Format: 1 1 1 1 A  (meaning: false :- not A, i.e., A must be true)
    - To force atom A to be false: add a constraint with body = {A}
      Format: 1 1 1 0 A  (meaning: false :- A, i.e., A must be false)

    Actually, in smodels format:
    - Integrity constraint: 1 1 <num_lits> <num_neg> <neg_lits...> <pos_lits...>
      where head=1 means the constraint head (false)

    Wait, let me check the format more carefully...
    """
    lines = smodels.strip().split('\n')

    # Find where rules end (first '0' line)
    rules_end_idx = None
    for i, line in enumerate(lines):
        if line.strip() == '0':
            rules_end_idx = i
            break

    if rules_end_idx is None:
        print("Could not find end of rules in smodels")
        sys.exit(1)

    # Insert integrity constraints before the '0'
    new_rules = []
    for atom_id, is_true in enumerate(candidate_model, start=1):
        if is_true:
            # Force atom to be true: :- not atom.
            # smodels: 1 1 1 1 atom (type=1, head=1, 1 lit, 1 neg, neg_lit=atom)
            new_rules.append(f"1 1 1 1 {atom_id}")
        else:
            # Force atom to be false: :- atom.
            # smodels: 1 1 1 0 atom (type=1, head=1, 1 lit, 0 neg, pos_lit=atom)
            new_rules.append(f"1 1 1 0 {atom_id}")

    # Reconstruct smodels with new rules
    result_lines = lines[:rules_end_idx] + new_rules + lines[rules_end_idx:]
    return '\n'.join(result_lines)

def run_test(lp_file, constraint_idx):
    """Test if the candidate model from constraint N is a valid stable model."""
    print(f"Testing candidate model from constraint #{constraint_idx}")
    print()

    # Get gringo output
    print("1. Getting gringo output...")
    smodels = get_gringo_output(lp_file)
    max_atom = parse_smodels_max_atom(smodels)
    print(f"   Max atom ID: {max_atom}")

    # Get candidate model from our solver
    print(f"2. Running our solver to get candidate model for constraint #{constraint_idx}...")
    candidate_model = get_constraint_candidate_model(lp_file, constraint_idx)

    if candidate_model is None:
        print("   ERROR: Could not get candidate model. Need to add ASP_DUMP_CONSTRAINT support.")
        print("   Let's add that feature first.")
        return

    true_count = sum(1 for x in candidate_model if x)
    print(f"   Candidate model: {true_count} true atoms out of {len(candidate_model)}")

    # Add integrity constraints
    print("3. Adding integrity constraints...")
    constrained_smodels = add_integrity_constraints(smodels, candidate_model)

    # Run clasp
    print("4. Running clasp on constrained problem...")
    result = subprocess.run(
        ['clasp', '1'],
        input=constrained_smodels,
        capture_output=True, text=True
    )

    print()
    print("=== CLASP OUTPUT ===")
    print(result.stdout)

    if 'UNSATISFIABLE' in result.stdout:
        print("RESULT: UNSATISFIABLE")
        print("=> The candidate model is NOT a valid stable model.")
        print("=> We correctly rejected it. The loop constraint is correct.")
    elif 'SATISFIABLE' in result.stdout:
        print("RESULT: SATISFIABLE")
        print("=> The candidate model IS a valid stable model.")
        print("=> Our loop constraint incorrectly rejected it. BUG IN LOOP CONSTRAINT!")
    else:
        print("RESULT: UNKNOWN")

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("Usage: python3 test_candidate_model.py <file.lp> [constraint_idx]")
        print("  constraint_idx defaults to 151 (loop_constraints index for the first violation)")
        print("  Note: constraint at array index N corresponds to loop_constraints=N+1")
        sys.exit(1)

    lp_file = sys.argv[1]
    constraint_idx = int(sys.argv[2]) if len(sys.argv) > 2 else 151
    run_test(lp_file, constraint_idx)

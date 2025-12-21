#!/usr/bin/env python3
"""
Test that our integrity constraint infrastructure works correctly.

Takes a clasp answer and adds integrity constraints to force each atom to its value,
then runs clasp again. Should always be SATISFIABLE if the infrastructure is correct.
"""

import subprocess
import sys
import os

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

def get_clasp_answers(smodels):
    """Run clasp and get all answers as sets of atom names."""
    result = subprocess.run(
        ['clasp', '0'],
        input=smodels,
        capture_output=True, text=True
    )

    answers = []
    lines = result.stdout.split('\n')
    for i, line in enumerate(lines):
        if line.startswith('Answer:'):
            if i + 1 < len(lines):
                atoms_line = lines[i + 1].strip()
                atoms = set(atoms_line.split()) if atoms_line else set()
                answers.append(atoms)
    return answers

def parse_smodels_symbols(smodels):
    """Parse symbol table from smodels format, returns name->id and id->name mappings."""
    name_to_id = {}
    id_to_name = {}
    lines = smodels.split('\n')
    in_symbols = False
    rules_done = False

    for line in lines:
        line = line.strip()
        if not line:
            continue
        if line == '0':
            if not rules_done:
                rules_done = True
                in_symbols = True
            else:
                break
        elif in_symbols:
            parts = line.split(None, 1)
            if len(parts) >= 2:
                try:
                    atom_id = int(parts[0])
                    name = parts[1]
                    name_to_id[name] = atom_id
                    id_to_name[atom_id] = name
                except ValueError:
                    pass
    return name_to_id, id_to_name

def get_max_atom(smodels):
    """Find the maximum atom ID in the smodels output."""
    max_atom = 0
    for line in smodels.split('\n'):
        parts = line.split()
        if not parts:
            continue
        for part in parts:
            try:
                atom_id = int(part)
                if atom_id > max_atom:
                    max_atom = atom_id
            except ValueError:
                continue
    return max_atom

def answer_to_bool_vector(answer, name_to_id, max_atom):
    """Convert an answer (set of atom names) to a boolean vector."""
    # result[i] = True iff Atom(i+1) is in the answer
    result = [False] * max_atom

    for name in answer:
        if name in name_to_id:
            atom_id = name_to_id[name]
            if 1 <= atom_id <= max_atom:
                result[atom_id - 1] = True

    return result

def add_integrity_constraints(smodels, bool_vector):
    """
    Add integrity constraints to force each atom to its value.

    In smodels format:
    - To force atom A to be true: :- not A.
      Format: 1 1 1 1 A (type=1, head=1/false, 1 lit, 1 neg, neg_lit=A)
    - To force atom A to be false: :- A.
      Format: 1 1 1 0 A (type=1, head=1/false, 1 lit, 0 neg, pos_lit=A)
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
    for atom_id, is_true in enumerate(bool_vector, start=1):
        if is_true:
            # Force atom to be true: :- not atom.
            new_rules.append(f"1 1 1 1 {atom_id}")
        else:
            # Force atom to be false: :- atom.
            new_rules.append(f"1 1 1 0 {atom_id}")

    # Reconstruct smodels with new rules
    result_lines = lines[:rules_end_idx] + new_rules + lines[rules_end_idx:]
    return '\n'.join(result_lines)

def run_test(lp_file, answer_idx):
    """Test that clasp's answer N passes through integrity constraints."""
    print(f"Testing clasp answer #{answer_idx}")
    print()

    # Get gringo output with names for all atoms
    print("1. Getting gringo output (with names for all atoms)...")
    smodels = get_gringo_output(lp_file, add_names=True)
    max_atom = get_max_atom(smodels)
    name_to_id, id_to_name = parse_smodels_symbols(smodels)
    print(f"   Max atom ID: {max_atom}")
    print(f"   Symbol table has {len(name_to_id)} entries")

    # Get clasp answers
    print("2. Running clasp to get answers...")
    answers = get_clasp_answers(smodels)
    print(f"   clasp found {len(answers)} answers")

    if answer_idx < 1 or answer_idx > len(answers):
        print(f"   ERROR: answer_idx {answer_idx} out of range [1, {len(answers)}]")
        return

    answer = answers[answer_idx - 1]
    print(f"   Answer {answer_idx} has {len(answer)} atoms")

    # Convert to boolean vector
    bool_vector = answer_to_bool_vector(answer, name_to_id, max_atom)
    true_count = sum(1 for x in bool_vector if x)
    print(f"   Boolean vector: {true_count} true atoms out of {max_atom}")

    # Add integrity constraints
    print("3. Adding integrity constraints...")
    constrained_smodels = add_integrity_constraints(smodels, bool_vector)

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
        print("=> BUG IN TEST INFRASTRUCTURE! Clasp's own answer should be satisfiable!")
    elif 'SATISFIABLE' in result.stdout:
        print("RESULT: SATISFIABLE")
        print("=> Good! Test infrastructure works correctly.")
    else:
        print("RESULT: UNKNOWN")

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("Usage: python3 test_clasp_answer.py <file.lp> [answer_idx]")
        print("  answer_idx defaults to 6 (the first missing answer)")
        sys.exit(1)

    lp_file = sys.argv[1]
    answer_idx = int(sys.argv[2]) if len(sys.argv) > 2 else 6
    run_test(lp_file, answer_idx)

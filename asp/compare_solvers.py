#!/usr/bin/env python3
"""
Compare our ASP solver against clasp, checking if our loop constraints
are violated by clasp's valid solutions.

Usage: python3 compare_solvers.py asp/sorting_network.lp
"""

import subprocess
import sys
import re
import os

def parse_clasp_answers(output):
    """Parse clasp output to get list of answer sets (each is a set of atom names)."""
    answers = []
    lines = output.split('\n')
    for i, line in enumerate(lines):
        if line.startswith('Answer:'):
            # Next line contains the atoms
            if i + 1 < len(lines):
                atoms_line = lines[i + 1].strip()
                atoms = set(atoms_line.split()) if atoms_line else set()
                answers.append(atoms)
    return answers

def atom_name_to_id(name):
    """Convert atom name to ID. Returns None if not parseable."""
    if name.startswith('__atom_'):
        try:
            return int(name[7:])
        except ValueError:
            return None
    return None

def answer_to_atom_ids(answer, symbol_table):
    """Convert answer set (atom names) to set of atom IDs."""
    atom_ids = set()
    for name in answer:
        if name.startswith('__atom_'):
            atom_id = atom_name_to_id(name)
            if atom_id:
                atom_ids.add(atom_id)
        elif name in symbol_table:
            atom_ids.add(symbol_table[name])
    return atom_ids

def parse_smodels_symbols(smodels):
    """Parse symbol table from smodels format, returns name->id mapping."""
    symbol_table = {}
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
                    symbol_table[name] = atom_id
                except ValueError:
                    pass
    return symbol_table

def run_comparison(lp_file):
    # Step 1: Run gringo to get smodels format
    print(f"Running gringo on {lp_file}...")
    gringo = subprocess.run(
        ['gringo', '--output=smodels', lp_file],
        capture_output=True, text=True
    )
    if gringo.returncode != 0:
        print(f"gringo error: {gringo.stderr}")
        return

    smodels_raw = gringo.stdout

    # Step 2: Add names for all atoms
    print("Adding names for internal atoms...")
    name_script = os.path.join(os.path.dirname(__file__), 'name_all_atoms.py')
    name_proc = subprocess.run(
        ['python3', name_script],
        input=smodels_raw,
        capture_output=True, text=True
    )
    smodels_named = name_proc.stdout

    # Parse symbol table from named smodels
    symbol_table = parse_smodels_symbols(smodels_named)
    print(f"Symbol table has {len(symbol_table)} entries")

    # Step 3: Run clasp to get reference answers
    print("Running clasp...")
    clasp = subprocess.run(
        ['clasp', '0'],
        input=smodels_named,
        capture_output=True, text=True
    )
    clasp_answers = parse_clasp_answers(clasp.stdout)
    print(f"clasp found {len(clasp_answers)} answers")

    # Convert to atom ID sets
    clasp_atom_sets = []
    for i, ans in enumerate(clasp_answers):
        atom_ids = answer_to_atom_ids(ans, symbol_table)
        clasp_atom_sets.append(atom_ids)
        print(f"  Answer {i+1}: {len(atom_ids)} atoms")

    # Step 4: Run our solver with constraint recording enabled
    print("\nRunning our solver...")
    env = os.environ.copy()
    env['ASP_DEBUG'] = '1'

    our_solver = subprocess.run(
        ['./target/release/asp', '0'],
        input=smodels_raw,  # Use raw smodels (without synthetic names)
        capture_output=True, text=True,
        cwd=os.path.dirname(os.path.dirname(__file__)) or '.',
        env=env
    )

    print("Our solver output (stderr):")
    for line in our_solver.stderr.split('\n')[:20]:
        print(f"  {line}")

    # Parse our answers
    our_answers = []
    for line in our_solver.stdout.split('\n'):
        if line.startswith('Answer'):
            # Format: "Answer N: atom1 atom2 ..."
            parts = line.split(':', 1)
            if len(parts) == 2:
                atoms = set(parts[1].strip().split()) if parts[1].strip() else set()
                our_answers.append(atoms)

    print(f"\nOur solver found {len(our_answers)} answers")

    # Compare answers
    print("\n=== COMPARISON ===")

    # Find clasp answers missing from ours
    our_comp_sets = []
    for ans in our_answers:
        # Extract just the comp predicates for comparison
        comps = {a for a in ans if a.startswith('comp(')}
        our_comp_sets.append(comps)

    clasp_comp_sets = []
    for ans in clasp_answers:
        comps = {a for a in ans if a.startswith('comp(')}
        clasp_comp_sets.append(comps)

    missing = []
    for i, clasp_comps in enumerate(clasp_comp_sets):
        found = False
        for our_comps in our_comp_sets:
            if clasp_comps == our_comps:
                found = True
                break
        if not found:
            missing.append(i)
            print(f"MISSING: clasp answer {i+1}")
            print(f"  comp predicates: {sorted(clasp_comps)}")

    if not missing:
        print("All clasp answers found by our solver!")
        return

    # For each missing answer, run verification
    print("\n=== VERIFYING MISSING ANSWERS AGAINST OUR CONSTRAINTS ===")
    for idx in missing:
        print(f"\n{'='*60}")
        print(f"Missing answer {idx+1}:")
        clasp_ans = clasp_answers[idx]
        print(f"  Full model has {len(clasp_ans)} atom names")

        # Pass all atom names (including __atom_N) to our solver for verification
        atom_names_str = ' '.join(sorted(clasp_ans))

        env = os.environ.copy()
        env['ASP_VERIFY'] = atom_names_str

        verify_run = subprocess.run(
            ['./target/release/asp', '0'],
            input=smodels_raw,
            capture_output=True, text=True,
            cwd=os.path.dirname(os.path.dirname(__file__)) or '.',
            env=env
        )

        # Print verification output
        print("\n  Verification output:")
        for line in verify_run.stderr.split('\n'):
            if line.startswith('c '):
                print(f"  {line}")

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("Usage: python3 compare_solvers.py <file.lp>")
        sys.exit(1)
    run_comparison(sys.argv[1])

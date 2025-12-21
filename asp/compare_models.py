#!/usr/bin/env python3
"""Compare our candidate model with clasp's answer to see where they differ."""

import subprocess
import sys
import os

def get_gringo_output(lp_file, add_names=False):
    result = subprocess.run(
        ['gringo', '--output=smodels', lp_file],
        capture_output=True, text=True
    )
    if result.returncode != 0:
        print(f"gringo error: {result.stderr}")
        sys.exit(1)
    smodels = result.stdout
    if add_names:
        script_dir = os.path.dirname(__file__)
        name_script = os.path.join(script_dir, 'name_all_atoms.py')
        result = subprocess.run(['python3', name_script], input=smodels, capture_output=True, text=True)
        smodels = result.stdout
    return smodels

def get_candidate_model(lp_file, constraint_idx):
    smodels = get_gringo_output(lp_file)
    env = os.environ.copy()
    env['ASP_DUMP_CONSTRAINT'] = str(constraint_idx)
    result = subprocess.run(['./target/release/asp', '0'], input=smodels, capture_output=True, text=True, env=env)
    for line in result.stderr.split('\n'):
        if line.startswith('c CANDIDATE_MODEL:'):
            bits = line.split(':')[1].strip()
            return [c == '1' for c in bits]
    return None

def get_clasp_answer(lp_file, answer_idx):
    smodels = get_gringo_output(lp_file, add_names=True)
    result = subprocess.run(['clasp', '0'], input=smodels, capture_output=True, text=True)
    answers = []
    lines = result.stdout.split('\n')
    for i, line in enumerate(lines):
        if line.startswith('Answer:'):
            if i + 1 < len(lines):
                atoms = set(lines[i + 1].strip().split())
                answers.append(atoms)
    if answer_idx < 1 or answer_idx > len(answers):
        return None
    # Convert to bool vector
    answer = answers[answer_idx - 1]
    max_atom = 300  # Known from earlier
    result = [False] * max_atom
    for name in answer:
        if name.startswith('__atom_'):
            try:
                atom_id = int(name[7:])
                if 1 <= atom_id <= max_atom:
                    result[atom_id - 1] = True
            except:
                pass
    return result

def parse_symbols(lp_file):
    smodels = get_gringo_output(lp_file, add_names=True)
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
                    id_to_name[atom_id] = name
                except:
                    pass
    return id_to_name

if __name__ == '__main__':
    lp_file = sys.argv[1] if len(sys.argv) > 1 else 'asp/sorting_network.lp'
    constraint_idx = int(sys.argv[2]) if len(sys.argv) > 2 else 151
    answer_idx = int(sys.argv[3]) if len(sys.argv) > 3 else 6

    print(f"Comparing candidate model (constraint #{constraint_idx}) with clasp answer #{answer_idx}")
    print()

    cand = get_candidate_model(lp_file, constraint_idx)
    clasp = get_clasp_answer(lp_file, answer_idx)
    symbols = parse_symbols(lp_file)

    if cand is None:
        print("Could not get candidate model")
        sys.exit(1)
    if clasp is None:
        print("Could not get clasp answer")
        sys.exit(1)

    print(f"Candidate: {sum(cand)} true atoms")
    print(f"Clasp:     {sum(clasp)} true atoms")
    print()

    # Find differences
    only_in_cand = []
    only_in_clasp = []
    for i in range(len(cand)):
        atom_id = i + 1
        name = symbols.get(atom_id, f"atom_{atom_id}")
        if cand[i] and not clasp[i]:
            only_in_cand.append((atom_id, name))
        elif clasp[i] and not cand[i]:
            only_in_clasp.append((atom_id, name))

    print(f"Atoms only in CANDIDATE ({len(only_in_cand)}):")
    for atom_id, name in sorted(only_in_cand):
        print(f"  {atom_id}: {name}")
    print()
    print(f"Atoms only in CLASP ({len(only_in_clasp)}):")
    for atom_id, name in sorted(only_in_clasp):
        print(f"  {atom_id}: {name}")

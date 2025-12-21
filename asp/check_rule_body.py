#!/usr/bin/env python3
"""Check specific atoms in clasp answers."""

import subprocess
import sys
import os

def get_named_smodels(lp_file):
    script_dir = os.path.dirname(__file__)
    name_script = os.path.join(script_dir, 'name_all_atoms.py')

    gringo = subprocess.run(
        ['gringo', '--output=smodels', lp_file],
        capture_output=True, text=True
    )

    named = subprocess.run(
        ['python3', name_script],
        input=gringo.stdout,
        capture_output=True, text=True
    )
    return named.stdout

def parse_symbols(smodels):
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

def get_clasp_answer(smodels, answer_idx):
    result = subprocess.run(
        ['clasp', '0'],
        input=smodels,
        capture_output=True, text=True
    )

    lines = result.stdout.split('\n')
    for i, line in enumerate(lines):
        if line.startswith(f'Answer: {answer_idx}'):
            if i + 1 < len(lines):
                return set(lines[i + 1].strip().split())
    return None

def main():
    lp_file = sys.argv[1] if len(sys.argv) > 1 else 'asp/sorting_network.lp'
    answer_idx = int(sys.argv[2]) if len(sys.argv) > 2 else 6
    atoms_to_check = [int(a) for a in sys.argv[3:]] if len(sys.argv) > 3 else [213, 214, 215, 216, 152, 268]

    print(f"Checking atoms {atoms_to_check} in clasp answer #{answer_idx}")
    print()

    smodels = get_named_smodels(lp_file)
    id_to_name = parse_symbols(smodels)
    answer = get_clasp_answer(smodels, answer_idx)

    if answer is None:
        print(f"Could not find answer {answer_idx}")
        return

    print(f"Answer #{answer_idx} has {len(answer)} atoms")
    print()

    for atom_id in atoms_to_check:
        name = id_to_name.get(atom_id, f"__atom_{atom_id}")
        in_answer = name in answer or f"__atom_{atom_id}" in answer
        print(f"  Atom({atom_id}) = {name}: {'TRUE' if in_answer else 'FALSE'}")

if __name__ == '__main__':
    main()

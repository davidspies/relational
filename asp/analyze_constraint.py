#!/usr/bin/env python3
"""Analyze a specific UFS constraint, mapping each term to its meaning."""

import subprocess
import sys
import os
import re

def get_gringo_output(lp_file):
    result = subprocess.run(
        ['gringo', '--output=smodels', lp_file],
        capture_output=True, text=True
    )
    return result.stdout

def get_named_gringo_output(lp_file):
    script_dir = os.path.dirname(__file__)
    name_script = os.path.join(script_dir, 'name_all_atoms.py')

    smodels = get_gringo_output(lp_file)
    result = subprocess.run(
        ['python3', name_script],
        input=smodels,
        capture_output=True, text=True
    )
    return result.stdout

def parse_symbols(smodels):
    """Parse symbol table: atom_id -> name"""
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

def get_constraint_terms(lp_file, constraint_idx):
    """Get constraint terms from ASP_DUMP_CONSTRAINT output."""
    smodels = get_gringo_output(lp_file)
    env = os.environ.copy()
    env['ASP_DUMP_CONSTRAINT'] = str(constraint_idx)

    result = subprocess.run(
        ['./target/release/asp', '0'],
        input=smodels,
        capture_output=True, text=True,
        env=env
    )

    # Parse: c CONSTRAINT_TERMS: [(Lit(-152), 1), (Lit(2254), 1), ...]
    for line in result.stderr.split('\n'):
        if line.startswith('c CONSTRAINT_TERMS:'):
            terms_str = line.split(':', 1)[1].strip()
            return terms_str
    return None

def parse_terms(terms_str):
    """Parse [(Lit(-152), 1), (Lit(2254), 1), ...] into list of (lit, weight)"""
    terms = []
    # Find all Lit(...) patterns
    pattern = r'\(Lit\((-?\d+)\),\s*(\d+)\)'
    for match in re.finditer(pattern, terms_str):
        lit = int(match.group(1))
        weight = int(match.group(2))
        terms.append((lit, weight))
    return terms

def get_var_layout_info(lp_file):
    """Get variable layout info from the solver."""
    smodels = get_gringo_output(lp_file)
    env = os.environ.copy()
    env['ASP_DUMP_LAYOUT'] = '1'

    result = subprocess.run(
        ['./target/release/asp', '0'],
        input=smodels,
        capture_output=True, text=True,
        env=env
    )

    layout = {}
    for line in result.stderr.split('\n'):
        if line.startswith('c LAYOUT:'):
            # Parse layout info
            pass
    return layout

def main():
    lp_file = sys.argv[1] if len(sys.argv) > 1 else 'asp/sorting_network.lp'
    constraint_idx = int(sys.argv[2]) if len(sys.argv) > 2 else 150

    print(f"Analyzing constraint #{constraint_idx} for {lp_file}")
    print()

    # Get atom names
    named_smodels = get_named_gringo_output(lp_file)
    id_to_name = parse_symbols(named_smodels)

    print(f"Total atoms with names: {len(id_to_name)}")

    # Get constraint terms
    terms_str = get_constraint_terms(lp_file, constraint_idx)
    if not terms_str:
        print(f"Could not get constraint #{constraint_idx}")
        return

    terms = parse_terms(terms_str)
    print(f"Constraint has {len(terms)} terms:")
    print()

    # Analyze each term
    for i, (lit, weight) in enumerate(terms):
        var = abs(lit)
        negated = lit < 0
        sign = "¬" if negated else ""

        # Try to identify what this variable represents
        if var in id_to_name:
            name = id_to_name[var]
            print(f"  Term {i+1}: {sign}Var({var}) = {sign}{name}, weight={weight}")
        else:
            # Must be an encoding variable (active_cand, used_cand, active_head_cand, etc.)
            print(f"  Term {i+1}: {sign}Var({var}) = [encoding var], weight={weight}")

    print()
    print("Legend:")
    print("  - Atoms 1..N are x_cand variables (true iff atom in model)")
    print("  - Higher numbered vars are active_cand, used_cand, active_head_cand")
    print("  - For UFS constraints: typically ¬chosen_atom + Σ active_head vars >= 1")

if __name__ == '__main__':
    main()

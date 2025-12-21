#!/usr/bin/env python3
"""Trace an atom back to its dependencies, showing the derivation chain."""

import subprocess
import sys
import os
from collections import defaultdict

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

def parse_smodels(smodels):
    """Parse smodels format into rules and symbol table."""
    rules = []
    symbols = {}  # atom_id -> name
    lines = smodels.split('\n')
    in_rules = True
    in_symbols = False

    for line in lines:
        line = line.strip()
        if not line:
            continue

        if line == '0':
            if in_rules:
                in_rules = False
                in_symbols = True
            else:
                break
            continue

        if in_rules:
            parts = list(map(int, line.split()))
            rule_type = parts[0]
            if rule_type == 1:  # basic rule
                head = parts[1]
                num_lits = parts[2]
                num_neg = parts[3]
                body = parts[4:4+num_lits]
                neg_body = body[:num_neg]
                pos_body = body[num_neg:]
                rules.append({
                    'type': 'basic',
                    'head': head,
                    'pos_body': pos_body,
                    'neg_body': neg_body
                })
            elif rule_type == 2:  # cardinality/count rule
                head = parts[1]
                num_lits = parts[2]
                num_neg = parts[3]
                bound = parts[4]
                body = parts[5:5+num_lits]
                neg_body = body[:num_neg]
                pos_body = body[num_neg:]
                rules.append({
                    'type': 'count',
                    'head': head,
                    'pos_body': pos_body,
                    'neg_body': neg_body,
                    'bound': bound
                })
            elif rule_type == 3:  # choice rule
                head_count = parts[1]
                heads = parts[2:2+head_count]
                rest = parts[2+head_count:]
                num_lits = rest[0]
                num_neg = rest[1]
                body = rest[2:2+num_lits]
                neg_body = body[:num_neg]
                pos_body = body[num_neg:]
                for h in heads:
                    rules.append({
                        'type': 'choice',
                        'head': h,
                        'pos_body': pos_body,
                        'neg_body': neg_body
                    })
            elif rule_type == 5:  # weight rule
                head = parts[1]
                bound = parts[2]
                num_lits = parts[3]
                num_neg = parts[4]
                rest = parts[5:]
                # Each lit has (atom, weight) pairs
                weighted = []
                for i in range(num_lits):
                    atom = rest[2*i]
                    weight = rest[2*i + 1]
                    weighted.append((atom, weight))
                neg_weighted = weighted[:num_neg]
                pos_weighted = weighted[num_neg:]
                rules.append({
                    'type': 'weight',
                    'head': head,
                    'pos_weighted': pos_weighted,
                    'neg_weighted': neg_weighted,
                    'bound': bound
                })
        elif in_symbols:
            parts = line.split(None, 1)
            if len(parts) >= 2:
                try:
                    atom_id = int(parts[0])
                    name = parts[1]
                    symbols[atom_id] = name
                except:
                    pass

    return rules, symbols

def trace_atom(atom_id, rules, symbols, depth=0, visited=None):
    """Recursively trace an atom's derivation."""
    if visited is None:
        visited = set()

    if atom_id in visited:
        return
    visited.add(atom_id)

    indent = "  " * depth
    name = symbols.get(atom_id, f"__atom_{atom_id}")

    # Find rules that derive this atom
    deriving_rules = [r for r in rules if r['head'] == atom_id]

    if not deriving_rules:
        print(f"{indent}Atom {atom_id} ({name}): NO RULES (fact or external)")
        return

    for i, rule in enumerate(deriving_rules):
        if rule['type'] == 'basic':
            pos = rule['pos_body']
            neg = rule['neg_body']
            pos_str = ", ".join(symbols.get(a, f"__atom_{a}") for a in pos)
            neg_str = ", ".join(f"not {symbols.get(a, f'__atom_{a}')}" for a in neg)
            body_parts = [p for p in [pos_str, neg_str] if p]
            body = ", ".join(body_parts) if body_parts else "⊤"
            print(f"{indent}Atom {atom_id} ({name}) ← Rule #{i}: {body}")

            # Recursively trace positive body atoms that are internal
            for a in pos:
                if symbols.get(a, '').startswith('__atom_'):
                    trace_atom(a, rules, symbols, depth + 1, visited)

        elif rule['type'] == 'count':
            pos = rule['pos_body']
            neg = rule['neg_body']
            bound = rule['bound']
            pos_str = ", ".join(symbols.get(a, f"__atom_{a}") for a in pos)
            neg_str = ", ".join(f"not {symbols.get(a, f'__atom_{a}')}" for a in neg)
            body_parts = [p for p in [pos_str, neg_str] if p]
            body = ", ".join(body_parts) if body_parts else "⊤"
            print(f"{indent}Atom {atom_id} ({name}) ← Rule #{i}: #{bound} {{ {body} }}")

            for a in pos:
                if symbols.get(a, '').startswith('__atom_'):
                    trace_atom(a, rules, symbols, depth + 1, visited)

        elif rule['type'] == 'choice':
            pos = rule['pos_body']
            neg = rule['neg_body']
            pos_str = ", ".join(symbols.get(a, f"__atom_{a}") for a in pos)
            neg_str = ", ".join(f"not {symbols.get(a, f'__atom_{a}')}" for a in neg)
            body_parts = [p for p in [pos_str, neg_str] if p]
            body = ", ".join(body_parts) if body_parts else "⊤"
            print(f"{indent}Atom {atom_id} ({name}) ← CHOICE #{i}: {body}")

            for a in pos:
                if symbols.get(a, '').startswith('__atom_'):
                    trace_atom(a, rules, symbols, depth + 1, visited)

        elif rule['type'] == 'weight':
            pos_w = rule['pos_weighted']
            neg_w = rule['neg_weighted']
            bound = rule['bound']
            pos_str = ", ".join(f"{w}:{symbols.get(a, f'__atom_{a}')}" for a, w in pos_w)
            neg_str = ", ".join(f"{w}:not {symbols.get(a, f'__atom_{a}')}" for a, w in neg_w)
            body_parts = [p for p in [pos_str, neg_str] if p]
            body = ", ".join(body_parts) if body_parts else "⊤"
            print(f"{indent}Atom {atom_id} ({name}) ← WEIGHT #{i}: #{bound} {{ {body} }}")

            for a, w in pos_w:
                if symbols.get(a, '').startswith('__atom_'):
                    trace_atom(a, rules, symbols, depth + 1, visited)

def main():
    lp_file = sys.argv[1] if len(sys.argv) > 1 else 'asp/sorting_network.lp'
    atom_id = int(sys.argv[2]) if len(sys.argv) > 2 else 152

    print(f"Tracing atom {atom_id} in {lp_file}")
    print()

    smodels = get_named_smodels(lp_file)
    rules, symbols = parse_smodels(smodels)

    print(f"Parsed {len(rules)} rules, {len(symbols)} symbols")
    print()

    trace_atom(atom_id, rules, symbols)

if __name__ == '__main__':
    main()

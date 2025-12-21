#!/usr/bin/env python3
"""Map each term in a constraint to its sorting network meaning."""

import subprocess
import sys
import os
import re
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
    return gringo.stdout, named.stdout

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

def parse_rules(smodels):
    """Parse smodels into rules with head -> bodies mapping."""
    rules = []
    head_to_rules = defaultdict(list)
    lines = smodels.split('\n')

    for line in lines:
        line = line.strip()
        if not line or line == '0':
            break

        parts = list(map(int, line.split()))
        rule_type = parts[0]

        if rule_type == 1:  # basic rule
            head = parts[1]
            num_lits = parts[2]
            num_neg = parts[3]
            body = parts[4:4+num_lits]
            rules.append({'type': 'basic', 'head': head, 'body': body, 'num_neg': num_neg})
            head_to_rules[head].append(len(rules) - 1)

        elif rule_type == 2:  # count rule
            head = parts[1]
            num_lits = parts[2]
            num_neg = parts[3]
            bound = parts[4]
            body = parts[5:5+num_lits]
            rules.append({'type': 'count', 'head': head, 'body': body, 'bound': bound, 'num_neg': num_neg})
            head_to_rules[head].append(len(rules) - 1)

        elif rule_type == 3:  # choice rule
            head_count = parts[1]
            heads = parts[2:2+head_count]
            rest = parts[2+head_count:]
            num_lits = rest[0]
            num_neg = rest[1]
            body = rest[2:2+num_lits]
            for h in heads:
                rules.append({'type': 'choice', 'head': h, 'body': body, 'num_neg': num_neg})
                head_to_rules[h].append(len(rules) - 1)

    return rules, head_to_rules

def trace_to_named(atom_id, id_to_name, rules, head_to_rules, depth=0, visited=None):
    """Trace an atom to its named form."""
    if visited is None:
        visited = set()

    if atom_id in visited:
        return f"[cycle: {atom_id}]"
    visited.add(atom_id)

    name = id_to_name.get(atom_id, f"__atom_{atom_id}")

    # If it has a real name (not __atom_), return it
    if not name.startswith('__atom_'):
        return name

    # Otherwise, trace through rules
    if atom_id not in head_to_rules:
        return name

    rule_idxs = head_to_rules[atom_id]
    if not rule_idxs:
        return name

    # Just use the first rule for simplicity
    rule = rules[rule_idxs[0]]
    body = rule['body']
    num_neg = rule['num_neg']
    neg_body = body[:num_neg]
    pos_body = body[num_neg:]

    pos_parts = [trace_to_named(b, id_to_name, rules, head_to_rules, depth+1, visited.copy()) for b in pos_body]
    neg_parts = [f"not {trace_to_named(b, id_to_name, rules, head_to_rules, depth+1, visited.copy())}" for b in neg_body]

    if rule['type'] == 'count':
        bound = rule['bound']
        all_parts = pos_parts + neg_parts
        return f"#{bound}{{{', '.join(all_parts)}}}"
    else:
        all_parts = pos_parts + neg_parts
        return f"({' & '.join(all_parts)})" if all_parts else "⊤"

def get_constraint_info(lp_file, constraint_idx):
    """Get constraint terms from solver."""
    raw_smodels, _ = get_named_smodels(lp_file)

    env = os.environ.copy()
    env['ASP_DUMP_CONSTRAINT'] = str(constraint_idx)

    result = subprocess.run(
        ['./target/release/asp', '0'],
        input=raw_smodels,
        capture_output=True, text=True,
        env=env
    )

    terms = []
    rules_info = {}

    for line in result.stderr.split('\n'):
        if 'Term' in line and 'ActiveHeadCand' in line:
            # Parse: c   Term N: Var(X) = ActiveHeadCand { rule_idx: R, head_idx: H, level: L }, weight=W
            match = re.search(r'Term (\d+):.*rule_idx: (\d+), head_idx: (\d+), level: (\d+)', line)
            if match:
                term_num = int(match.group(1))
                rule_idx = int(match.group(2))
                head_idx = int(match.group(3))
                level = int(match.group(4))
                terms.append((term_num, rule_idx, head_idx, level))

        if 'Term 1:' in line and 'Cand(Atom' in line:
            # Parse chosen atom
            match = re.search(r'Cand\(Atom\((\d+)\)\)', line)
            if match:
                chosen_atom = int(match.group(1))
                terms.insert(0, (1, 'chosen', chosen_atom, None))

        # Parse rule info
        if '<- Rule' in line:
            match = re.search(r'Atom\((\d+)\) <- Rule (\d+): (.+)', line)
            if match:
                head = int(match.group(1))
                rule_idx = int(match.group(2))
                rule_desc = match.group(3)
                if rule_idx not in rules_info:
                    rules_info[rule_idx] = {'head': head, 'desc': rule_desc}

    return terms, rules_info

def main():
    lp_file = sys.argv[1] if len(sys.argv) > 1 else 'asp/sorting_network.lp'
    constraint_idx = int(sys.argv[2]) if len(sys.argv) > 2 else 150

    print(f"Mapping constraint #{constraint_idx} terms to sorting network meaning")
    print("=" * 70)
    print()

    raw_smodels, named_smodels = get_named_smodels(lp_file)
    id_to_name = parse_symbols(named_smodels)
    rules, head_to_rules = parse_rules(raw_smodels)

    terms, rules_info = get_constraint_info(lp_file, constraint_idx)

    for term in terms:
        if term[1] == 'chosen':
            atom_id = term[2]
            name = id_to_name.get(atom_id, f"__atom_{atom_id}")
            traced = trace_to_named(atom_id, id_to_name, rules, head_to_rules)
            print(f"Term {term[0]}: ¬Atom({atom_id})")
            print(f"         = ¬{name}")
            print(f"         → {traced}")
            print()
        else:
            term_num, rule_idx, head_idx, level = term
            if rule_idx in rules_info:
                head = rules_info[rule_idx]['head']
                name = id_to_name.get(head, f"__atom_{head}")
                traced = trace_to_named(head, id_to_name, rules, head_to_rules)
                print(f"Term {term_num}: active_head(rule={rule_idx}, head={head_idx}, level={level})")
                print(f"         Head: Atom({head}) = {name}")
                print(f"         → {traced}")
                print()

if __name__ == '__main__':
    main()

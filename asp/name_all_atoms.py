#!/usr/bin/env python3
"""
Preprocessor for smodels format: adds names for all atoms without names.

Usage: gringo --output=smodels foo.lp | python3 name_all_atoms.py | clasp 0

This allows clasp to output all atoms (including gringo's internal auxiliaries)
so we can verify our solver's constraints against the complete model.
"""

import sys

def main():
    lines = sys.stdin.read().split('\n')

    # First pass: find all atoms used in rules and existing symbol table entries
    all_atoms = set()
    named_atoms = set()
    rules_section = []
    symbol_section = []
    rest_section = []

    section = 'rules'
    for line in lines:
        if section == 'rules':
            if line == '0':
                rules_section.append(line)
                section = 'symbols'
            else:
                rules_section.append(line)
                # Parse atoms from rule
                parts = line.split()
                if parts:
                    rule_type = parts[0] if parts else ''
                    # Extract atom IDs based on rule type
                    for p in parts[1:]:
                        try:
                            atom_id = int(p)
                            if atom_id > 1:  # Skip false atom (1)
                                all_atoms.add(atom_id)
                        except ValueError:
                            pass
        elif section == 'symbols':
            if line == '0':
                symbol_section.append(line)
                section = 'rest'
            else:
                symbol_section.append(line)
                # Parse existing symbol: "atom_id name"
                parts = line.split(None, 1)
                if len(parts) >= 1:
                    try:
                        atom_id = int(parts[0])
                        named_atoms.add(atom_id)
                    except ValueError:
                        pass
        else:
            rest_section.append(line)

    # Output rules section
    for line in rules_section:
        print(line)

    # Output existing symbols (without the trailing 0)
    for line in symbol_section[:-1]:
        print(line)

    # Add synthetic names for unnamed atoms
    unnamed = sorted(all_atoms - named_atoms)
    for atom_id in unnamed:
        print(f"{atom_id} __atom_{atom_id}")

    # Output the 0 terminator
    print('0')

    # Output rest of file
    for line in rest_section:
        print(line)

if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Verify if a clasp answer violates any UFS constraint."""

import subprocess
import sys
import os

def main():
    lp_file = sys.argv[1] if len(sys.argv) > 1 else 'asp/sorting_network.lp'
    answer_idx = int(sys.argv[2]) if len(sys.argv) > 2 else 6

    # Get gringo output with names
    script_dir = os.path.dirname(__file__)
    name_script = os.path.join(script_dir, 'name_all_atoms.py')

    gringo = subprocess.run(
        ['gringo', '--output=smodels', lp_file],
        capture_output=True, text=True
    )
    smodels_raw = gringo.stdout

    named = subprocess.run(
        ['python3', name_script],
        input=smodels_raw,
        capture_output=True, text=True
    )
    smodels_named = named.stdout

    # Get clasp answers
    clasp = subprocess.run(
        ['clasp', '0'],
        input=smodels_named,
        capture_output=True, text=True
    )

    # Parse answer N
    lines = clasp.stdout.split('\n')
    answer = None
    for i, line in enumerate(lines):
        if line.startswith(f'Answer: {answer_idx}'):
            if i + 1 < len(lines):
                answer = lines[i + 1].strip()
                break

    if answer is None:
        print(f"Could not find answer {answer_idx}")
        sys.exit(1)

    print(f"Clasp answer {answer_idx}: {len(answer.split())} atoms")

    # Run our solver with ASP_VERIFY set
    env = os.environ.copy()
    env['ASP_VERIFY'] = answer

    result = subprocess.run(
        ['./target/release/asp', '0'],
        input=smodels_raw,
        capture_output=True, text=True,
        env=env
    )

    # Print verification output
    for line in result.stderr.split('\n'):
        if line.startswith('c '):
            print(line)

if __name__ == '__main__':
    main()

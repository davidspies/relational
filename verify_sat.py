#!/usr/bin/env python3
"""Verify that a SAT solution satisfies all clauses in a CNF file."""

import sys


def parse_solution(output: str) -> set[int]:
    """Parse solver output, extracting literals from 'v' lines."""
    literals = set()
    for line in output.strip().split('\n'):
        line = line.strip()
        if not line.startswith('v '):
            continue
        parts = line[2:].split()
        for p in parts:
            val = int(p)
            if val != 0:
                literals.add(val)
    return literals


def parse_cnf(filename: str) -> list[list[int]]:
    """Parse a DIMACS CNF file into a list of clauses."""
    clauses = []
    with open(filename) as f:
        for line in f:
            line = line.strip()
            # Skip comments and problem line
            if not line or line.startswith('c') or line.startswith('p') or line.startswith('%'):
                continue
            # Parse clause: list of literals ending with 0
            lits = [int(x) for x in line.split()]
            if lits and lits[-1] == 0:
                lits = lits[:-1]
            if lits:
                clauses.append(lits)
    return clauses


def check_consistency(solution: set[int]) -> list[int]:
    """Return list of variables that appear both positive and negative."""
    return [lit for lit in solution if lit > 0 and -lit in solution]


def verify(cnf_file: str, solution: set[int]) -> tuple[bool, list[list[int]]]:
    """Verify solution against CNF. Returns (success, unsatisfied_clauses)."""
    clauses = parse_cnf(cnf_file)
    unsatisfied = []
    for clause in clauses:
        if not any(lit in solution for lit in clause):
            unsatisfied.append(clause)
    return len(unsatisfied) == 0, unsatisfied


def main():
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <cnf_file> < solution")
        sys.exit(1)

    cnf_file = sys.argv[1]
    solution_line = sys.stdin.read()

    solution = parse_solution(solution_line)

    contradictions = check_consistency(solution)
    if contradictions:
        print(f"s NOT VERIFIED (contradictory assignments for variables: {contradictions})")
        sys.exit(1)

    success, unsatisfied = verify(cnf_file, solution)

    if success:
        print("s VERIFIED")
    else:
        print(f"s NOT VERIFIED ({len(unsatisfied)} unsatisfied clauses)")
        for clause in unsatisfied[:10]:  # Show first 10
            print(f"  {clause}")
        if len(unsatisfied) > 10:
            print(f"  ... and {len(unsatisfied) - 10} more")
        sys.exit(1)


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Verify that a SAT solution satisfies all clauses in a CNF file."""

import sys


def parse_solution(solution_line: str) -> set[int]:
    """Parse a solution line like 'v -1 2 -3 ... 0' into a set of literals."""
    parts = solution_line.strip().split()
    if parts[0] == 'v':
        parts = parts[1:]
    # Remove trailing 0
    if parts and parts[-1] == '0':
        parts = parts[:-1]
    return set(int(x) for x in parts)


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


def verify(cnf_file: str, solution: set[int]) -> tuple[bool, list[list[int]]]:
    """Verify solution against CNF. Returns (success, unsatisfied_clauses)."""
    clauses = parse_cnf(cnf_file)
    unsatisfied = []
    for clause in clauses:
        if not any(lit in solution for lit in clause):
            unsatisfied.append(clause)
    return len(unsatisfied) == 0, unsatisfied


def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <cnf_file> <solution_line>")
        sys.exit(1)

    cnf_file = sys.argv[1]
    solution_line = sys.argv[2]

    solution = parse_solution(solution_line)
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

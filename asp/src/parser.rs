//! Parser for smodels/lparse format.

use crate::types::{Atom, BasicRule, ChoiceRule, DisjunctiveRule, Program, Rule};

/// Parse a program in smodels format.
pub fn parse_smodels(input: &str) -> Result<Program, String> {
    let mut lines = input.lines().peekable();
    let mut program = Program::new();
    let mut max_atom = 1u32; // Atom 1 is reserved for "false"

    // gringo's default output is aspif (header "asp 1 0 0"), which is a different
    // format entirely. Reject it rather than misparsing it as smodels.
    if let Some(line) = lines.peek()
        && line.trim().starts_with("asp ")
    {
        return Err(
            "Input is in aspif format, not smodels. Run gringo with `--output=smodels`."
                .to_string(),
        );
    }

    // Parse rules until we hit "0"
    for line in lines.by_ref() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "0" {
            break;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let rule_type: u32 = parts[0]
            .parse()
            .map_err(|e| format!("Invalid rule type: {e}"))?;

        match rule_type {
            1 => {
                // Basic rule: 1 head body_count neg_count body_lits...
                let rule = parse_basic_rule(&parts)?;
                max_atom = max_atom.max(rule.head.0);
                for &atom in &rule.pos_body {
                    max_atom = max_atom.max(atom.0);
                }
                for &atom in &rule.neg_body {
                    max_atom = max_atom.max(atom.0);
                }
                program.rules.push(Rule::Basic(rule));
            }
            3 => {
                // Choice rule: 3 head_count heads... body_count neg_count body_lits...
                let rule = parse_choice_rule(&parts)?;
                for &atom in &rule.heads {
                    max_atom = max_atom.max(atom.0);
                }
                for &atom in &rule.pos_body {
                    max_atom = max_atom.max(atom.0);
                }
                for &atom in &rule.neg_body {
                    max_atom = max_atom.max(atom.0);
                }
                program.rules.push(Rule::Choice(rule));
            }
            8 => {
                // Disjunctive rule: 8 head_count heads... body_count neg_count body_lits...
                let rule = parse_disjunctive_rule(&parts)?;
                for &atom in &rule.heads {
                    max_atom = max_atom.max(atom.0);
                }
                for &atom in &rule.pos_body {
                    max_atom = max_atom.max(atom.0);
                }
                for &atom in &rule.neg_body {
                    max_atom = max_atom.max(atom.0);
                }
                program.rules.push(Rule::Disjunctive(rule));
            }
            _ => {
                return Err(format!("Unsupported rule type: {rule_type}"));
            }
        }
    }

    // Parse symbol table until we hit "0"
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "0" {
            break;
        }

        // Format: atom_id name
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() >= 2 {
            let atom_id: u32 = parts[0]
                .parse()
                .map_err(|e| format!("Invalid atom ID: {e}"))?;
            let name = parts[1].to_string();
            program.symbols.push((Atom(atom_id), name));
        }
    }

    // Skip B+, B-, and model count sections (not needed for solving)
    program.max_atom = max_atom;
    Ok(program)
}

fn parse_basic_rule(parts: &[&str]) -> Result<BasicRule, String> {
    // Format: 1 head body_count neg_count body_lits...
    if parts.len() < 4 {
        return Err("Basic rule too short".to_string());
    }

    let head = Atom(parts[1].parse().map_err(|e| format!("Invalid head: {e}"))?);
    let body_count: usize = parts[2]
        .parse()
        .map_err(|e| format!("Invalid body count: {e}"))?;
    let neg_count: usize = parts[3]
        .parse()
        .map_err(|e| format!("Invalid neg count: {e}"))?;

    if parts.len() < 4 + body_count {
        return Err(format!(
            "Body too short: expected {body_count} literals, got {}",
            parts.len() - 4
        ));
    }

    if neg_count > body_count {
        return Err(format!(
            "Negative literal count {neg_count} exceeds body count {body_count}"
        ));
    }

    let mut neg_body = Vec::with_capacity(neg_count);
    let mut pos_body = Vec::with_capacity(body_count - neg_count);

    for (i, &part) in parts[4..4 + body_count].iter().enumerate() {
        let atom = Atom(
            part.parse()
                .map_err(|e| format!("Invalid body literal: {e}"))?,
        );
        if i < neg_count {
            neg_body.push(atom);
        } else {
            pos_body.push(atom);
        }
    }

    Ok(BasicRule {
        head,
        pos_body,
        neg_body,
    })
}

fn parse_choice_rule(parts: &[&str]) -> Result<ChoiceRule, String> {
    // Format: 3 head_count heads... body_count neg_count body_lits...
    if parts.len() < 2 {
        return Err("Choice rule too short".to_string());
    }

    let head_count: usize = parts[1]
        .parse()
        .map_err(|e| format!("Invalid head count: {e}"))?;

    if parts.len() < 2 + head_count + 2 {
        return Err("Choice rule too short for heads".to_string());
    }

    let mut heads = Vec::with_capacity(head_count);
    for &part in &parts[2..2 + head_count] {
        heads.push(Atom(
            part.parse().map_err(|e| format!("Invalid head: {e}"))?,
        ));
    }

    let body_start = 2 + head_count;
    let body_count: usize = parts[body_start]
        .parse()
        .map_err(|e| format!("Invalid body count: {e}"))?;
    let neg_count: usize = parts[body_start + 1]
        .parse()
        .map_err(|e| format!("Invalid neg count: {e}"))?;

    let body_lits_start = body_start + 2;
    if parts.len() < body_lits_start + body_count {
        return Err(format!(
            "Body too short: expected {body_count} literals, got {}",
            parts.len() - body_lits_start
        ));
    }

    if neg_count > body_count {
        return Err(format!(
            "Negative literal count {neg_count} exceeds body count {body_count}"
        ));
    }

    let mut neg_body = Vec::with_capacity(neg_count);
    let mut pos_body = Vec::with_capacity(body_count - neg_count);

    for (i, &part) in parts[body_lits_start..body_lits_start + body_count]
        .iter()
        .enumerate()
    {
        let atom = Atom(
            part.parse()
                .map_err(|e| format!("Invalid body literal: {e}"))?,
        );
        if i < neg_count {
            neg_body.push(atom);
        } else {
            pos_body.push(atom);
        }
    }

    Ok(ChoiceRule {
        heads,
        pos_body,
        neg_body,
    })
}

fn parse_disjunctive_rule(parts: &[&str]) -> Result<DisjunctiveRule, String> {
    // Format: 8 head_count heads... body_count neg_count body_lits...
    // Same format as choice rules
    if parts.len() < 2 {
        return Err("Disjunctive rule too short".to_string());
    }

    let head_count: usize = parts[1]
        .parse()
        .map_err(|e| format!("Invalid head count: {e}"))?;

    if parts.len() < 2 + head_count + 2 {
        return Err("Disjunctive rule too short for heads".to_string());
    }

    let mut heads = Vec::with_capacity(head_count);
    for &part in &parts[2..2 + head_count] {
        heads.push(Atom(
            part.parse().map_err(|e| format!("Invalid head: {e}"))?,
        ));
    }

    let body_start = 2 + head_count;
    let body_count: usize = parts[body_start]
        .parse()
        .map_err(|e| format!("Invalid body count: {e}"))?;
    let neg_count: usize = parts[body_start + 1]
        .parse()
        .map_err(|e| format!("Invalid neg count: {e}"))?;

    let body_lits_start = body_start + 2;
    if parts.len() < body_lits_start + body_count {
        return Err(format!(
            "Body too short: expected {body_count} literals, got {}",
            parts.len() - body_lits_start
        ));
    }

    if neg_count > body_count {
        return Err(format!(
            "Negative literal count {neg_count} exceeds body count {body_count}"
        ));
    }

    let mut neg_body = Vec::with_capacity(neg_count);
    let mut pos_body = Vec::with_capacity(body_count - neg_count);

    for (i, &part) in parts[body_lits_start..body_lits_start + body_count]
        .iter()
        .enumerate()
    {
        let atom = Atom(
            part.parse()
                .map_err(|e| format!("Invalid body literal: {e}"))?,
        );
        if i < neg_count {
            neg_body.push(atom);
        } else {
            pos_body.push(atom);
        }
    }

    Ok(DisjunctiveRule {
        heads,
        pos_body,
        neg_body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let input = r#"
1 2 1 1 3
1 3 1 1 2
0
2 b
3 a
0
B+
0
B-
1
0
1
"#;
        let program = parse_smodels(input).unwrap();
        assert_eq!(program.rules.len(), 2);
        assert_eq!(program.symbols.len(), 2);
    }

    #[test]
    fn test_parse_choice() {
        let input = r#"
3 1 2 0 0
1 3 1 0 2
1 4 1 0 3
0
2 p
3 q
4 r
0
"#;
        let program = parse_smodels(input).unwrap();
        assert_eq!(program.rules.len(), 3);
        assert!(matches!(&program.rules[0], Rule::Choice(_)));
    }

    #[test]
    fn test_parse_disjunctive() {
        let input = r#"
8 2 2 3 0 0
0
2 a
3 b
0
"#;
        let program = parse_smodels(input).unwrap();
        assert_eq!(program.rules.len(), 1);
        assert!(matches!(&program.rules[0], Rule::Disjunctive(_)));
        if let Rule::Disjunctive(r) = &program.rules[0] {
            assert_eq!(r.heads.len(), 2);
            assert_eq!(r.heads[0], Atom(2));
            assert_eq!(r.heads[1], Atom(3));
        }
    }
}

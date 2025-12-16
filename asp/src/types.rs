//! ASP types for atoms, rules, and programs.

/// Weight for PB constraints.
pub type Weight = i32;

/// An atom ID (non-zero positive integer in smodels).
/// Atom 1 is reserved for "false" (contradiction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Atom(pub u32);

impl Atom {
    /// The special "false" atom - if this becomes true, we have a contradiction.
    pub const FALSE: Atom = Atom(1);

    pub fn is_false(self) -> bool {
        self == Self::FALSE
    }
}

/// A literal is an atom with a sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lit {
    /// Positive = atom must be true, negative = atom must be false (default negation).
    atom: Atom,
    positive: bool,
}

impl Lit {
    pub fn pos(atom: Atom) -> Self {
        Self {
            atom,
            positive: true,
        }
    }

    pub fn neg(atom: Atom) -> Self {
        Self {
            atom,
            positive: false,
        }
    }

    pub fn atom(self) -> Atom {
        self.atom
    }

    pub fn is_positive(self) -> bool {
        self.positive
    }

    pub fn is_negative(self) -> bool {
        !self.positive
    }
}

/// A weighted literal: atom with a weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeightedLit {
    pub atom: Atom,
    pub positive: bool,
    pub weight: Weight,
}

impl WeightedLit {
    pub fn pos(atom: Atom, weight: Weight) -> Self {
        Self {
            atom,
            positive: true,
            weight,
        }
    }

    pub fn neg(atom: Atom, weight: Weight) -> Self {
        Self {
            atom,
            positive: false,
            weight,
        }
    }
}

/// A choice rule: {heads} :- body
#[derive(Debug, Clone)]
pub struct ChoiceRule {
    pub heads: Vec<Atom>,
    pub body: Vec<WeightedLit>,
    /// The bound/threshold for the body to be satisfied.
    pub bound: Weight,
}

/// A disjunctive rule: head1 | head2 | ... :- body
#[derive(Debug, Clone)]
pub struct DisjunctiveRule {
    pub heads: Vec<Atom>,
    pub body: Vec<WeightedLit>,
    /// The bound/threshold for the body to be satisfied.
    pub bound: Weight,
}

/// A rule in the program.
#[derive(Debug, Clone)]
pub enum Rule {
    Choice(ChoiceRule),
    Disjunctive(DisjunctiveRule),
}

/// A grounded ASP program in smodels format.
#[derive(Debug, Clone, Default)]
pub struct Program {
    pub rules: Vec<Rule>,
    /// Symbol table: atom -> name
    pub symbols: Vec<(Atom, String)>,
    /// Maximum atom ID used.
    pub max_atom: u32,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }
}

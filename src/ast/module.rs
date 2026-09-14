use super::Statement;

/// A module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    statements: Vec<Statement>,
}

impl Module {
    /// Creates a module.
    pub const fn new(statements: Vec<Statement>) -> Self {
        Self { statements }
    }

    /// Returns statements.
    pub fn statements(&self) -> &[Statement] {
        &self.statements
    }
}

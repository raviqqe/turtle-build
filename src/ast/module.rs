use super::Statement;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    statements: Vec<Statement>,
}

impl Module {
    pub fn new(statements: Vec<Statement>) -> Self {
        Self { statements }
    }

    pub fn statements(&self) -> &[Statement] {
        &self.statements
    }
}

use super::Statement;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module<'a> {
    statements: Vec<Statement<'a>>,
}

impl<'a> Module<'a> {
    pub fn new(statements: Vec<Statement<'a>>) -> Self {
        Self { statements }
    }

    pub fn statements(&self) -> &[Statement<'a>] {
        &self.statements
    }
}

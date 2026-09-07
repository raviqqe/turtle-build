use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepfileError {
    path: String,
    line: usize,
    column: usize,
    message: String,
}

impl DepfileError {
    pub fn new(
        path: impl Into<String>,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            line,
            column,
            message: message.into(),
        }
    }
}

impl Error for DepfileError {}

impl Display for DepfileError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}: {}",
            self.path, self.line, self.column, self.message
        )
    }
}

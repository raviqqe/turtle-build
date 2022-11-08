#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule<'a> {
    name: &'a str,
    command: &'a str,
    description: Option<&'a str>,
}

impl<'a> Rule<'a> {
    pub fn new(name: &'a str, command: &'a str, description: Option<&'a str>) -> Self {
        Self {
            name,
            command,
            description,
        }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn command(&self) -> &str {
        self.command
    }

    pub fn description(&self) -> Option<&str> {
        self.description
    }
}

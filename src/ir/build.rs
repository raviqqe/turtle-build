#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    id: String,
    command: String,
    description: String,
    inputs: Vec<String>,
    order_only_inputs: Vec<String>,
}

impl Build {
    pub fn new(
        id: impl Into<String>,
        command: impl Into<String>,
        description: impl Into<String>,
        inputs: Vec<String>,
        order_only_inputs: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            command: command.into(),
            description: description.into(),
            inputs,
            order_only_inputs,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    #[allow(dead_code)]
    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }

    pub fn order_only_inputs(&self) -> &[String] {
        &self.order_only_inputs
    }
}

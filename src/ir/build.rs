#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    id: String,
    command: Option<String>,
    description: String,
    inputs: Vec<String>,
    order_only_inputs: Vec<String>,
}

impl Build {
    pub fn new(
        id: impl Into<String>,
        command: Option<String>,
        description: impl Into<String>,
        inputs: Vec<String>,
        order_only_inputs: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            command,
            description: description.into(),
            inputs,
            order_only_inputs,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
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

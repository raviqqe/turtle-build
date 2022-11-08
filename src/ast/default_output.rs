#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultOutput<'a> {
    outputs: Vec<&'a str>,
}

impl<'a> DefaultOutput<'a> {
    pub fn new(outputs: Vec<&'a str>) -> Self {
        Self { outputs }
    }

    pub fn outputs(&self) -> &[&'a str] {
        &self.outputs
    }
}

use super::DynamicBuild;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicConfiguration<'a> {
    outputs: HashMap<&'a str, DynamicBuild<'a>>,
}

impl<'a> DynamicConfiguration<'a> {
    pub fn new(outputs: HashMap<&'a str, DynamicBuild<'a>>) -> Self {
        Self { outputs }
    }

    pub fn outputs(&self) -> &HashMap<&'a str, DynamicBuild<'a>> {
        &self.outputs
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicBuild<'a> {
    inputs: Vec<&'a str>,
}

impl<'a> DynamicBuild<'a> {
    pub fn new(inputs: Vec<&'a str>) -> Self {
        Self { inputs }
    }

    pub fn inputs(&self) -> &[&str] {
        &self.inputs
    }
}

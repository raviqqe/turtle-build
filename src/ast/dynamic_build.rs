#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynamicBuild<'a> {
    output: &'a str,
    implicit_inputs: Vec<&'a str>,
}

impl<'a> DynamicBuild<'a> {
    pub fn new(output: &'a str, implicit_inputs: Vec<&'a str>) -> Self {
        Self {
            output,
            implicit_inputs,
        }
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn implicit_inputs(&self) -> &[&str] {
        &self.implicit_inputs
    }
}

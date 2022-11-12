use super::Rule;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct BuildId(u64);

impl BuildId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn to_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(u64::from_le_bytes(bytes))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    // IDs are persistent across different builds so that they can be used for,
    // for example, caching.
    id: BuildId,
    outputs: Vec<Arc<str>>,
    implicit_outputs: Vec<Arc<str>>,
    rule: Option<Rule>,
    inputs: Vec<Arc<str>>,
    order_only_inputs: Vec<Arc<str>>,
    dynamic_module: Option<Arc<str>>,
}

impl Build {
    pub fn new(
        outputs: Vec<Arc<str>>,
        implicit_outputs: Vec<Arc<str>>,
        rule: Option<Rule>,
        inputs: Vec<Arc<str>>,
        order_only_inputs: Vec<Arc<str>>,
        dynamic_module: Option<Arc<str>>,
    ) -> Self {
        Self {
            id: Self::calculate_id(&outputs, &implicit_outputs),
            outputs,
            implicit_outputs,
            rule,
            inputs,
            order_only_inputs,
            dynamic_module,
        }
    }

    pub fn id(&self) -> BuildId {
        self.id
    }

    pub fn outputs(&self) -> &[Arc<str>] {
        &self.outputs
    }

    pub fn implicit_outputs(&self) -> &[Arc<str>] {
        &self.implicit_outputs
    }

    pub fn rule(&self) -> Option<&Rule> {
        self.rule.as_ref()
    }

    pub fn inputs(&self) -> &[Arc<str>] {
        &self.inputs
    }

    pub fn order_only_inputs(&self) -> &[Arc<str>] {
        &self.order_only_inputs
    }

    pub fn dynamic_module(&self) -> Option<&Arc<str>> {
        self.dynamic_module.as_ref()
    }

    fn calculate_id(outputs: &[Arc<str>], implicit_outputs: &[Arc<str>]) -> BuildId {
        let mut hasher = DefaultHasher::new();

        outputs.hash(&mut hasher);
        implicit_outputs.hash(&mut hasher);

        BuildId::new(hasher.finish())
    }
}

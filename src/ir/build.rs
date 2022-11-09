use super::{PathId, Rule};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    // IDs are persistent across different builds so that they can be used for,
    // for example, caching.
    id: BuildId,
    outputs: Vec<PathId>,
    implicit_outputs: Vec<PathId>,
    rule: Option<Rule>,
    inputs: Vec<PathId>,
    order_only_inputs: Vec<PathId>,
    dynamic_module: Option<String>,
}

impl Build {
    pub fn new(
        outputs: Vec<PathId>,
        implicit_outputs: Vec<PathId>,
        rule: Option<Rule>,
        inputs: Vec<PathId>,
        order_only_inputs: Vec<PathId>,
        dynamic_module: Option<String>,
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

    pub fn outputs(&self) -> &[PathId] {
        &self.outputs
    }

    pub fn implicit_outputs(&self) -> &[PathId] {
        &self.implicit_outputs
    }

    pub fn rule(&self) -> Option<&Rule> {
        self.rule.as_ref()
    }

    pub fn inputs(&self) -> &[PathId] {
        &self.inputs
    }

    pub fn order_only_inputs(&self) -> &[PathId] {
        &self.order_only_inputs
    }

    pub fn dynamic_module(&self) -> Option<&str> {
        self.dynamic_module.as_deref()
    }

    fn calculate_id(outputs: &[PathId], implicit_outputs: &[PathId]) -> BuildId {
        let mut hasher = DefaultHasher::new();

        outputs.hash(&mut hasher);
        implicit_outputs.hash(&mut hasher);

        BuildId::new(hasher.finish())
    }
}

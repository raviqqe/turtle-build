use super::Rule;
use crate::path_pool::FilePath;
use alloc::sync::Arc;
use core::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct BuildId(u64);

impl BuildId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn to_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    // IDs are persistent across different builds so that they can be used for,
    // for example, caching.
    id: BuildId,
    outputs: Vec<FilePath>,
    implicit_outputs: Vec<FilePath>,
    rule: Option<Rule>,
    inputs: Vec<FilePath>,
    order_only_inputs: Vec<FilePath>,
    dynamic_module: Option<FilePath>,
}

impl Build {
    pub fn new(
        outputs: Vec<FilePath>,
        implicit_outputs: Vec<FilePath>,
        rule: Option<Rule>,
        inputs: Vec<FilePath>,
        order_only_inputs: Vec<FilePath>,
        dynamic_module: Option<FilePath>,
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

    pub const fn id(&self) -> BuildId {
        self.id
    }

    pub fn outputs(&self) -> &[FilePath] {
        &self.outputs
    }

    pub fn implicit_outputs(&self) -> &[FilePath] {
        &self.implicit_outputs
    }

    pub const fn rule(&self) -> Option<&Rule> {
        self.rule.as_ref()
    }

    pub fn inputs(&self) -> &[FilePath] {
        &self.inputs
    }

    pub fn order_only_inputs(&self) -> &[FilePath] {
        &self.order_only_inputs
    }

    pub const fn dynamic_module(&self) -> Option<&FilePath> {
        self.dynamic_module.as_ref()
    }

    fn calculate_id(outputs: &[FilePath], implicit_outputs: &[FilePath]) -> BuildId {
        let mut hasher = DefaultHasher::new();

        outputs.hash(&mut hasher);
        implicit_outputs.hash(&mut hasher);

        BuildId::new(hasher.finish())
    }
}

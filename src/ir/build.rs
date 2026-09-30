use super::Rule;
use crate::stable_hasher::StableHasher;
use alloc::sync::Arc;
use core::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct BuildId(u64);

impl BuildId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(u64::from_le_bytes(bytes))
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

    pub const fn id(&self) -> BuildId {
        self.id
    }

    pub fn outputs(&self) -> &[Arc<str>] {
        &self.outputs
    }

    pub fn implicit_outputs(&self) -> &[Arc<str>] {
        &self.implicit_outputs
    }

    pub const fn rule(&self) -> Option<&Rule> {
        self.rule.as_ref()
    }

    pub fn inputs(&self) -> &[Arc<str>] {
        &self.inputs
    }

    pub fn order_only_inputs(&self) -> &[Arc<str>] {
        &self.order_only_inputs
    }

    pub const fn dynamic_module(&self) -> Option<&Arc<str>> {
        self.dynamic_module.as_ref()
    }

    fn calculate_id(outputs: &[Arc<str>], implicit_outputs: &[Arc<str>]) -> BuildId {
        let mut hasher = StableHasher::default();

        outputs.hash(&mut hasher);
        implicit_outputs.hash(&mut hasher);

        BuildId::new(hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};

    fn create_build(
        outputs: Vec<Arc<str>>,
        implicit_outputs: Vec<Arc<str>>,
        inputs: Vec<Arc<str>>,
    ) -> Build {
        Build::new(outputs, implicit_outputs, None, inputs, vec![], None)
    }

    #[test]
    fn convert_id_from_bytes() {
        assert_eq!(
            BuildId::from_bytes([0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]),
            BuildId::new(0x0102_0304_0506_0708)
        );
    }

    #[test]
    fn convert_id_to_bytes() {
        assert_eq!(
            BuildId::new(0x0102_0304_0506_0708).to_bytes(),
            [0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]
        );
    }

    #[test]
    fn convert_id_to_bytes_and_back() {
        for id in [0, 1, 42, u64::MAX] {
            assert_eq!(
                BuildId::from_bytes(BuildId::new(id).to_bytes()),
                BuildId::new(id)
            );
        }
    }

    #[test]
    fn calculate_id_from_outputs() {
        assert_eq!(
            create_build(vec!["foo".into()], vec![], vec![]).id(),
            create_build(vec!["foo".into()], vec![], vec![]).id()
        );
        assert_ne!(
            create_build(vec!["foo".into()], vec![], vec![]).id(),
            create_build(vec!["bar".into()], vec![], vec![]).id()
        );
    }

    #[test]
    fn calculate_id_from_implicit_outputs() {
        assert_ne!(
            create_build(vec!["foo".into()], vec![], vec![]).id(),
            create_build(vec!["foo".into()], vec!["bar".into()], vec![]).id()
        );
    }

    #[test]
    fn ignore_inputs_in_id() {
        assert_eq!(
            create_build(vec!["foo".into()], vec![], vec![]).id(),
            create_build(vec!["foo".into()], vec![], vec!["bar".into()]).id()
        );
    }
}

use crate::ir::{Build, BuildId, Config};
use alloc::sync::Arc;
use std::collections::HashMap;

pub fn sort_builds(config: &Config, outputs: &[Arc<Build>]) -> HashMap<BuildId, usize> {
    let mut sequences = HashMap::new();
    let mut stack = outputs.iter().rev().collect::<Vec<_>>();

    while let Some(build) = stack.pop() {
        if sequences.contains_key(&build.id()) {
            continue;
        }

        sequences.insert(build.id(), sequences.len());
        stack.extend(
            build
                .inputs()
                .iter()
                .chain(build.order_only_inputs())
                .rev()
                .filter_map(|input| config.outputs().get(input)),
        );
    }

    sequences
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Rule;
    use core::slice::from_ref;
    use pretty_assertions::assert_eq;

    fn create_build(output: &str, inputs: &[&str], order_only_inputs: &[&str]) -> Arc<Build> {
        Build::new(
            vec![output.into()],
            vec![],
            Some(Rule::new(format!("touch {output}"), None)),
            inputs.iter().map(|&input| input.into()).collect(),
            order_only_inputs
                .iter()
                .map(|&input| input.into())
                .collect(),
            None,
        )
        .into()
    }

    fn create_config(builds: &[&Arc<Build>]) -> Config {
        Config::new(
            builds
                .iter()
                .map(|&build| (build.outputs()[0].clone(), build.clone()))
                .collect(),
            Default::default(),
            Default::default(),
            Default::default(),
            None,
        )
    }

    #[test]
    fn order_outputs() {
        let foo = create_build("foo", &[], &[]);
        let bar = create_build("bar", &[], &[]);

        assert_eq!(
            sort_builds(&create_config(&[&foo, &bar]), &[foo.clone(), bar.clone()]),
            [(foo.id(), 0), (bar.id(), 1)].into_iter().collect()
        );
    }

    #[test]
    fn order_inputs_before_next_output() {
        let foo = create_build("foo", &["baz"], &[]);
        let bar = create_build("bar", &[], &[]);
        let baz = create_build("baz", &[], &[]);

        assert_eq!(
            sort_builds(
                &create_config(&[&foo, &bar, &baz]),
                &[foo.clone(), bar.clone()]
            ),
            [(foo.id(), 0), (baz.id(), 1), (bar.id(), 2)]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn order_inputs() {
        let foo = create_build("foo", &["bar", "baz"], &[]);
        let bar = create_build("bar", &[], &[]);
        let baz = create_build("baz", &[], &[]);

        assert_eq!(
            sort_builds(&create_config(&[&foo, &bar, &baz]), from_ref(&foo)),
            [(foo.id(), 0), (bar.id(), 1), (baz.id(), 2)]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn order_order_only_inputs_after_inputs() {
        let foo = create_build("foo", &["baz"], &["bar"]);
        let bar = create_build("bar", &[], &[]);
        let baz = create_build("baz", &[], &[]);

        assert_eq!(
            sort_builds(&create_config(&[&foo, &bar, &baz]), from_ref(&foo)),
            [(foo.id(), 0), (baz.id(), 1), (bar.id(), 2)]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn order_shared_input_once() {
        let foo = create_build("foo", &["baz"], &[]);
        let bar = create_build("bar", &["baz"], &[]);
        let baz = create_build("baz", &[], &[]);

        assert_eq!(
            sort_builds(
                &create_config(&[&foo, &bar, &baz]),
                &[foo.clone(), bar.clone()]
            ),
            [(foo.id(), 0), (baz.id(), 1), (bar.id(), 2)]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn skip_source_input() {
        let foo = create_build("foo", &["foo.c"], &[]);

        assert_eq!(
            sort_builds(&create_config(&[&foo]), from_ref(&foo)),
            [(foo.id(), 0)].into_iter().collect()
        );
    }
}

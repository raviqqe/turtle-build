use crate::{
    ast::Module,
    ir::{Build, Configuration, Rule},
};
use std::{collections::HashMap, sync::Arc};

pub fn compile(module: &Module) -> Result<Configuration, String> {
    let variables = module
        .variable_definitions()
        .iter()
        .map(|definition| (definition.name(), definition.value()))
        .collect::<HashMap<_, _>>();
    let rules = module
        .rules()
        .iter()
        .map(|rule| {
            (
                rule.name(),
                Arc::new(Rule::new(
                    interpolate_variables(rule.command(), &variables),
                    rule.description(),
                )),
            )
        })
        .collect::<HashMap<_, _>>();

    Ok(Configuration::new(
        module
            .builds()
            .iter()
            .flat_map(|build| {
                let ir = Arc::new(Build::new(
                    rules[build.rule()].clone(),
                    build.inputs().to_vec(),
                ));

                build
                    .outputs()
                    .iter()
                    .map(|output| (output.clone(), ir.clone()))
                    .collect::<Vec<_>>()
            })
            .collect(),
    ))
}

fn interpolate_variables(template: &str, variables: &HashMap<&str, &str>) -> String {
    // TODO
    template.into()
}

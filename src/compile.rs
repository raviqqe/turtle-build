use crate::{
    ast::Module,
    ir::{Build, Configuration},
};
use std::{collections::HashMap, sync::Arc};

pub fn compile(
    modules: &HashMap<String, Module>,
    root_module_path: &str,
) -> Result<Configuration, String> {
    let module = &modules[root_module_path];
    let mut build_index = 0;

    let variables = [("$", "$".into())]
        .into_iter()
        .chain(
            module
                .variable_definitions()
                .iter()
                .map(|definition| (definition.name(), definition.value().into())),
        )
        .collect::<HashMap<_, _>>();

    let rules = module
        .rules()
        .iter()
        .map(|rule| (rule.name(), rule))
        .collect::<HashMap<_, _>>();

    Ok(Configuration::new(
        module
            .builds()
            .iter()
            .flat_map(|build| {
                let rule = rules[build.rule()];
                let variables = variables
                    .clone()
                    .into_iter()
                    .chain([
                        ("in", build.inputs().join(" ")),
                        ("out", build.outputs().join(" ")),
                    ])
                    .collect();
                let ir = Arc::new(Build::new(
                    {
                        let index = build_index;
                        build_index += 1;
                        index.to_string()
                    },
                    interpolate_variables(rule.command(), &variables),
                    interpolate_variables(rule.description(), &variables),
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

// TODO Use rsplit to prevent overlapped interpolation.
fn interpolate_variables(template: &str, variables: &HashMap<&str, String>) -> String {
    variables
        .iter()
        .fold(template.into(), |template, (name, value)| {
            template.replace(&("$".to_string() + name), value)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ast, ir};
    use pretty_assertions::assert_eq;

    const ROOT_MODULE_PATH: &str = "build.ninja";

    #[test]
    fn compile_empty_module() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.into(),
                    ast::Module::new(vec![], vec![], vec![], vec![])
                )]
                .into_iter()
                .collect(),
                ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(Default::default())
        );
    }

    #[test]
    fn interpolate_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.into(),
                    ast::Module::new(
                        vec![ast::VariableDefinition::new("x", "42")],
                        vec![ast::Rule::new("foo", "$x", "")],
                        vec![ast::Build::new(vec!["bar".into()], "foo", vec![])],
                        vec![]
                    )
                )]
                .into_iter()
                .collect(),
                ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [("bar".into(), Build::new("0", "42", "", vec![]).into())]
                    .into_iter()
                    .collect()
            )
        );
    }

    #[test]
    fn interpolate_dollar_sign_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.into(),
                    ast::Module::new(
                        vec![ast::VariableDefinition::new("x", "42")],
                        vec![ast::Rule::new("foo", "$$", "")],
                        vec![ast::Build::new(vec!["bar".into()], "foo", vec![])],
                        vec![]
                    )
                )]
                .into_iter()
                .collect(),
                ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [("bar".into(), Build::new("0", "$", "", vec![]).into())]
                    .into_iter()
                    .collect()
            )
        );
    }

    #[test]
    fn interpolate_in_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.into(),
                    ast::Module::new(
                        vec![ast::VariableDefinition::new("x", "42")],
                        vec![ast::Rule::new("foo", "$in", "")],
                        vec![ast::Build::new(
                            vec!["bar".into()],
                            "foo",
                            vec!["baz".into()]
                        )],
                        vec![]
                    )
                )]
                .into_iter()
                .collect(),
                ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [(
                    "bar".into(),
                    Build::new("0", "baz", "", vec!["baz".into()]).into()
                )]
                .into_iter()
                .collect()
            )
        );
    }

    #[test]
    fn interpolate_out_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.into(),
                    ast::Module::new(
                        vec![ast::VariableDefinition::new("x", "42")],
                        vec![ast::Rule::new("foo", "$out", "")],
                        vec![ast::Build::new(vec!["bar".into()], "foo", vec![])],
                        vec![]
                    )
                )]
                .into_iter()
                .collect(),
                ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [("bar".into(), Build::new("0", "bar", "", vec![]).into())]
                    .into_iter()
                    .collect()
            )
        );
    }

    #[test]
    fn generate_build_ids() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.into(),
                    ast::Module::new(
                        vec![],
                        vec![ast::Rule::new("foo", "", "")],
                        vec![
                            ast::Build::new(vec!["bar".into()], "foo", vec![]),
                            ast::Build::new(vec!["baz".into()], "foo", vec![])
                        ],
                        vec![]
                    )
                )]
                .into_iter()
                .collect(),
                ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [
                    ("bar".into(), Build::new("0", "", "", vec![]).into()),
                    ("baz".into(), Build::new("1", "", "", vec![]).into())
                ]
                .into_iter()
                .collect()
            )
        );
    }
}

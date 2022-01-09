mod compiled_module;
mod context;

pub use self::context::ModuleDependencyMap;
use self::{compiled_module::CompiledModule, context::CompileContext};
use crate::{
    ast,
    ir::{Build, Configuration},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

pub fn compile(
    modules: &HashMap<PathBuf, ast::Module>,
    dependencies: &ModuleDependencyMap,
    root_module_path: &Path,
) -> Result<Configuration, String> {
    let context = CompileContext::new(modules.clone(), dependencies.clone());
    let rules = Default::default();
    let variables = [("$".into(), "$".into())].into_iter().collect();
    let module = compile_module(&context, root_module_path, &rules, &variables);

    let default_outputs = if module.default_outputs.is_empty() {
        module.outputs.keys().cloned().collect()
    } else {
        module.default_outputs
    };

    Ok(Configuration::new(module.outputs, default_outputs))
}

fn compile_module(
    context: &CompileContext,
    path: &Path,
    rules: &HashMap<String, ast::Rule>,
    variables: &HashMap<String, String>,
) -> CompiledModule {
    let module = &context.modules()[path];
    let mut rules = rules.clone();
    let mut variables = variables.clone();
    let mut outputs = HashMap::new();
    let mut default_outputs = HashSet::new();

    for statement in module.statements() {
        match statement {
            ast::Statement::Build(build) => {
                let rule = &rules[build.rule()];
                let variables =
                    variables
                        .clone()
                        .into_iter()
                        .chain(build.variable_definitions().iter().map(|definition| {
                            (definition.name().into(), definition.value().into())
                        }))
                        .chain([
                            ("in".into(), build.inputs().join(" ")),
                            ("out".into(), build.outputs().join(" ")),
                        ])
                        .collect();
                let ir = Arc::new(Build::new(
                    context.generate_build_id(),
                    interpolate_variables(rule.command(), &variables),
                    interpolate_variables(rule.description(), &variables),
                    build.inputs().to_vec(),
                ));

                outputs.extend(
                    build
                        .outputs()
                        .iter()
                        .map(|output| (output.clone(), ir.clone())),
                );
            }
            ast::Statement::Default(default) => {
                default_outputs.extend(default.outputs().iter().cloned());
            }
            ast::Statement::Include(include) => {
                let submodule = compile_module(
                    context,
                    &context.dependencies()[path][include.path()],
                    &rules,
                    &variables,
                );

                outputs.extend(submodule.outputs);
                default_outputs.extend(submodule.default_outputs);
                variables = submodule.variables;
                rules = submodule.rules;
            }
            ast::Statement::Rule(rule) => {
                rules.insert(rule.name().into(), rule.clone());
            }
            ast::Statement::Submodule(submodule) => {
                let submodule = compile_module(
                    context,
                    &context.dependencies()[path][submodule.path()],
                    &rules,
                    &variables,
                );

                outputs.extend(submodule.outputs);
                default_outputs.extend(submodule.default_outputs);
            }
            ast::Statement::VariableDefinition(definition) => {
                variables.insert(definition.name().into(), definition.value().into());
            }
        }
    }

    CompiledModule {
        outputs,
        default_outputs,
        rules,
        variables,
    }
}

// TODO Use rsplit to prevent overlapped interpolation.
fn interpolate_variables(template: &str, variables: &HashMap<String, String>) -> String {
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
    use once_cell::sync::Lazy;
    use pretty_assertions::assert_eq;

    static ROOT_MODULE_PATH: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("build.ninja"));
    static DEFAULT_DEPENDENCIES: Lazy<ModuleDependencyMap> = Lazy::new(|| {
        [(PathBuf::from("build.ninja"), Default::default())]
            .into_iter()
            .collect()
    });

    #[test]
    fn compile_empty_module() {
        assert_eq!(
            compile(
                &[(ROOT_MODULE_PATH.clone(), ast::Module::new(vec![]))]
                    .into_iter()
                    .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(Default::default(), Default::default())
        );
    }

    #[test]
    fn interpolate_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "42").into(),
                        ast::Rule::new("foo", "$x", "").into(),
                        ast::Build::new(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [("bar".into(), Build::new("0", "42", "", vec![]).into())]
                    .into_iter()
                    .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_dollar_sign_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "42").into(),
                        ast::Rule::new("foo", "$$", "").into(),
                        ast::Build::new(vec!["bar".into()], "foo", vec![], vec![]).into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [("bar".into(), Build::new("0", "$", "", vec![]).into())]
                    .into_iter()
                    .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_in_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "42").into(),
                        ast::Rule::new("foo", "$in", "").into(),
                        ast::Build::new(vec!["bar".into()], "foo", vec!["baz".into()], vec![])
                            .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [(
                    "bar".into(),
                    Build::new("0", "baz", "", vec!["baz".into()]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_out_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "42").into(),
                        ast::Rule::new("foo", "$out", "").into(),
                        ast::Build::new(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [("bar".into(), Build::new("0", "bar", "", vec![]).into())]
                    .into_iter()
                    .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn generate_build_ids() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::Rule::new("foo", "", "").into(),
                        ast::Build::new(vec!["bar".into()], "foo", vec![], vec![]).into(),
                        ast::Build::new(vec!["baz".into()], "foo", vec![], vec![]).into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [
                    ("bar".into(), Build::new("0", "", "", vec![]).into()),
                    ("baz".into(), Build::new("1", "", "", vec![]).into())
                ]
                .into_iter()
                .collect(),
                ["bar".into(), "baz".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_build_local_variable() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::Rule::new("foo", "$x", "").into(),
                        ast::Build::new(
                            vec!["bar".into()],
                            "foo",
                            vec![],
                            vec![ast::VariableDefinition::new("x", "42")]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            ir::Configuration::new(
                [("bar".into(), Build::new("0", "42", "", vec![]).into())]
                    .into_iter()
                    .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }
}

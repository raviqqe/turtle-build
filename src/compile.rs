mod context;

use self::context::CompileContext;
pub use self::context::ModuleDependencyMap;
use crate::{
    ast,
    ir::{Build, Configuration},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub fn compile(
    modules: &HashMap<PathBuf, ast::Module>,
    dependencies: &ModuleDependencyMap,
    root_module_path: &Path,
) -> Result<Configuration, String> {
    let outputs = compile_module(
        &CompileContext::new(modules.clone(), dependencies.clone()),
        root_module_path,
        &Default::default(),
        &[("$", "$".into())].into_iter().collect(),
    );
    let default_outputs = outputs.keys().cloned().collect();

    Ok(Configuration::new(outputs, default_outputs))
}

fn compile_module(
    context: &CompileContext,
    path: &Path,
    rules: &HashMap<&str, &ast::Rule>,
    variables: &HashMap<&str, String>,
) -> HashMap<String, Arc<Build>> {
    let module = &context.modules()[path];
    let mut rules = rules.clone();
    let mut variables = variables.clone();
    let mut outputs = HashMap::new();

    for statement in module.statements() {
        match statement {
            ast::Statement::Build(build) => {
                let rule = &rules[build.rule()];
                let variables = variables
                    .clone()
                    .into_iter()
                    .chain([
                        ("in", build.inputs().join(" ")),
                        ("out", build.outputs().join(" ")),
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
            ast::Statement::Default(_) => {}
            ast::Statement::Rule(rule) => {
                rules.insert(rule.name(), rule);
            }
            ast::Statement::Submodule(submodule) => {
                outputs.extend(compile_module(
                    context,
                    &context.dependencies()[path][submodule.path()],
                    &rules,
                    &variables,
                ));
            }
            ast::Statement::VariableDefinition(definition) => {
                variables.insert(definition.name(), definition.value().into());
            }
        }
    }

    outputs
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
                        ast::Build::new(vec!["bar".into()], "foo", vec![]).into(),
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
                        ast::Build::new(vec!["bar".into()], "foo", vec![]).into()
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
                        ast::Build::new(vec!["bar".into()], "foo", vec!["baz".into()]).into(),
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
                        ast::Build::new(vec!["bar".into()], "foo", vec![]).into(),
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
                        ast::Build::new(vec!["bar".into()], "foo", vec![]).into(),
                        ast::Build::new(vec!["baz".into()], "foo", vec![]).into()
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
}

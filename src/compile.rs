mod compiled_module;
mod context;

pub use self::context::ModuleDependencyMap;
use self::{compiled_module::CompiledModule, context::CompileContext};
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
    let context = CompileContext::new(modules.clone(), dependencies.clone());
    let mut state = CompiledModule {
        outputs: Default::default(),
        default_outputs: Default::default(),
        rules: Default::default(),
        variables: [("$".into(), "$".into())].into_iter().collect(),
    };
    compile_module(&context, &mut state, root_module_path);

    let default_outputs = if state.default_outputs.is_empty() {
        state.outputs.keys().cloned().collect()
    } else {
        state.default_outputs
    };

    Ok(Configuration::new(state.outputs, default_outputs))
}

fn compile_module(context: &CompileContext, state: &mut CompiledModule, path: &Path) {
    let module = &context.modules()[path];

    for statement in module.statements() {
        match statement {
            ast::Statement::Build(build) => {
                let rule = &state.rules[build.rule()];
                let local_variables = build
                    .variable_definitions()
                    .iter()
                    .map(|definition| (definition.name().into(), definition.value().into()))
                    .chain([
                        ("in".into(), build.inputs().join(" ")),
                        ("out".into(), build.outputs().join(" ")),
                    ])
                    .collect();

                let ir = Arc::new(Build::new(
                    context.generate_build_id(),
                    interpolate_variables(
                        &interpolate_variables(rule.command(), &local_variables),
                        &state.variables,
                    ),
                    interpolate_variables(
                        &interpolate_variables(rule.description(), &local_variables),
                        &state.variables,
                    ),
                    build.inputs().to_vec(),
                ));

                state.outputs.extend(
                    build
                        .outputs()
                        .iter()
                        .map(|output| (output.clone(), ir.clone())),
                );
            }
            ast::Statement::Default(default) => {
                state
                    .default_outputs
                    .extend(default.outputs().iter().cloned());
            }
            ast::Statement::Include(include) => {
                compile_module(
                    context,
                    state,
                    &context.dependencies()[path][include.path()],
                );
            }
            ast::Statement::Rule(rule) => {
                state.rules.insert(rule.name().into(), rule.clone());
            }
            ast::Statement::Submodule(submodule) => {
                // TODO
                compile_module(
                    context,
                    state,
                    &context.dependencies()[path][submodule.path()],
                );
            }
            ast::Statement::VariableDefinition(definition) => {
                state
                    .variables
                    .insert(definition.name().into(), definition.value().into());
            }
        }
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
        [(ROOT_MODULE_PATH.clone(), Default::default())]
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

    mod submodule {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn reference_variable_in_parent_module() {
            const SUBMODULE_PATH: &str = "foo.ninja";

            assert_eq!(
                compile(
                    &[
                        (
                            ROOT_MODULE_PATH.clone(),
                            ast::Module::new(vec![
                                ast::VariableDefinition::new("x", "42").into(),
                                ast::Submodule::new(SUBMODULE_PATH).into(),
                            ])
                        ),
                        (
                            SUBMODULE_PATH.into(),
                            ast::Module::new(vec![
                                ast::Rule::new("foo", "$x", "").into(),
                                ast::Build::new(vec!["bar".into()], "foo", vec![], vec![]).into()
                            ])
                        )
                    ]
                    .into_iter()
                    .collect(),
                    &[(
                        ROOT_MODULE_PATH.clone(),
                        [(SUBMODULE_PATH.into(), PathBuf::from(SUBMODULE_PATH))]
                            .into_iter()
                            .collect()
                    )]
                    .into_iter()
                    .collect(),
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
        fn reference_rule_in_parent_module() {
            const SUBMODULE_PATH: &str = "foo.ninja";

            assert_eq!(
                compile(
                    &[
                        (
                            ROOT_MODULE_PATH.clone(),
                            ast::Module::new(vec![
                                ast::VariableDefinition::new("x", "42").into(),
                                ast::Rule::new("foo", "$x", "").into(),
                                ast::Submodule::new(SUBMODULE_PATH).into(),
                            ])
                        ),
                        (
                            SUBMODULE_PATH.into(),
                            ast::Module::new(vec![ast::Build::new(
                                vec!["bar".into()],
                                "foo",
                                vec![],
                                vec![]
                            )
                            .into()])
                        )
                    ]
                    .into_iter()
                    .collect(),
                    &[(
                        ROOT_MODULE_PATH.clone(),
                        [(SUBMODULE_PATH.into(), PathBuf::from(SUBMODULE_PATH))]
                            .into_iter()
                            .collect()
                    )]
                    .into_iter()
                    .collect(),
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
        fn do_not_overwrite_variable_in_parent_module() {
            const SUBMODULE_PATH: &str = "foo.ninja";

            assert_eq!(
                compile(
                    &[
                        (
                            ROOT_MODULE_PATH.clone(),
                            ast::Module::new(vec![
                                ast::VariableDefinition::new("x", "42").into(),
                                ast::Rule::new("foo", "$x", "").into(),
                                ast::Submodule::new(SUBMODULE_PATH).into(),
                                ast::Build::new(vec!["bar".into()], "foo", vec![], vec![]).into(),
                            ])
                        ),
                        (
                            SUBMODULE_PATH.into(),
                            ast::Module::new(vec![ast::VariableDefinition::new("x", "13").into(),])
                        )
                    ]
                    .into_iter()
                    .collect(),
                    &[(
                        ROOT_MODULE_PATH.clone(),
                        [(SUBMODULE_PATH.into(), PathBuf::from(SUBMODULE_PATH))]
                            .into_iter()
                            .collect()
                    )]
                    .into_iter()
                    .collect(),
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
}

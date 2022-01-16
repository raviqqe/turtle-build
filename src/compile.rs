mod build_id_calculator;
mod chain_map;
mod context;
mod error;
mod global_state;
mod module_state;

use self::{
    build_id_calculator::BuildIdCalculator, chain_map::ChainMap, context::Context,
    global_state::GlobalState, module_state::ModuleState,
};
pub use self::{context::ModuleDependencyMap, error::CompileError};
use crate::{
    ast,
    ir::{Build, Configuration, DynamicBuild, DynamicConfiguration, Rule},
};
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

const PHONY_RULE: &str = "phony";
const BUILD_DIRECTORY_VARIABLE: &str = "builddir";
const DYNAMIC_MODULE_VARIABLE: &str = "dyndep";

static VARIABLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$([[:alpha:]_][[:alnum:]_]*)").unwrap());

pub fn compile(
    modules: &HashMap<PathBuf, ast::Module>,
    dependencies: &ModuleDependencyMap,
    root_module_path: &Path,
) -> Result<Configuration, CompileError> {
    let context = Context::new(modules.clone(), dependencies.clone());

    let mut global_state = GlobalState {
        outputs: Default::default(),
        default_outputs: Default::default(),
    };
    let mut module_state = ModuleState {
        rules: ChainMap::new(),
        variables: ChainMap::new(),
    };

    compile_module(
        &context,
        &mut global_state,
        &mut module_state,
        root_module_path,
    )?;

    let default_outputs = if global_state.default_outputs.is_empty() {
        global_state.outputs.keys().cloned().collect()
    } else {
        global_state.default_outputs
    };

    Ok(Configuration::new(
        global_state.outputs,
        default_outputs,
        module_state
            .variables
            .get(BUILD_DIRECTORY_VARIABLE)
            .cloned(),
    ))
}

fn compile_module(
    context: &Context,
    global_state: &mut GlobalState,
    module_state: &mut ModuleState,
    path: &Path,
) -> Result<(), CompileError> {
    let module = &context
        .modules()
        .get(path)
        .ok_or_else(|| CompileError::ModuleNotFound(path.into()))?;
    let mut build_id_calculator = BuildIdCalculator::new(path.into());

    for statement in module.statements() {
        match statement {
            ast::Statement::Build(build) => {
                let mut variables = module_state.variables.fork();

                variables.extend(
                    build
                        .variable_definitions()
                        .iter()
                        .map(|definition| (definition.name().into(), definition.value().into()))
                        .chain([
                            ("in".into(), build.inputs().join(" ")),
                            ("out".into(), build.outputs().join(" ")),
                        ]),
                );

                let ir = Arc::new(Build::new(
                    build_id_calculator.calculate(),
                    build.outputs().to_vec(),
                    build.implicit_outputs().to_vec(),
                    if build.rule() == PHONY_RULE {
                        None
                    } else {
                        let rule = &module_state
                            .rules
                            .get(build.rule())
                            .ok_or_else(|| CompileError::RuleNotFound(build.rule().into()))?;

                        Some(Rule::new(
                            interpolate_variables(rule.command(), &variables),
                            rule.description()
                                .map(|description| interpolate_variables(description, &variables)),
                        ))
                    },
                    build
                        .inputs()
                        .iter()
                        .chain(build.implicit_inputs())
                        .cloned()
                        .collect(),
                    build.order_only_inputs().to_vec(),
                    variables.get(DYNAMIC_MODULE_VARIABLE).cloned(),
                ));

                global_state.outputs.extend(
                    build
                        .outputs()
                        .iter()
                        .chain(build.implicit_outputs())
                        .map(|output| (output.clone(), ir.clone())),
                );
            }
            ast::Statement::Default(default) => {
                global_state
                    .default_outputs
                    .extend(default.outputs().iter().cloned());
            }
            ast::Statement::Include(include) => {
                compile_module(
                    context,
                    global_state,
                    module_state,
                    resolve_dependency(context, path, include.path())?,
                )?;
            }
            ast::Statement::Rule(rule) => {
                module_state.rules.insert(rule.name().into(), rule.clone());
            }
            ast::Statement::Submodule(submodule) => {
                compile_module(
                    context,
                    global_state,
                    &mut module_state.fork(),
                    resolve_dependency(context, path, submodule.path())?,
                )?;
            }
            ast::Statement::VariableDefinition(definition) => {
                module_state
                    .variables
                    .insert(definition.name().into(), definition.value().into());
            }
        }
    }

    Ok(())
}

pub fn compile_dynamic(module: &ast::DynamicModule) -> Result<DynamicConfiguration, CompileError> {
    Ok(DynamicConfiguration::new(
        module
            .builds()
            .iter()
            .map(|build| {
                (
                    build.output().into(),
                    DynamicBuild::new(build.implicit_inputs().to_vec()),
                )
            })
            .collect(),
    ))
}

fn resolve_dependency<'a>(
    context: &'a Context,
    module_path: &Path,
    submodule_path: &str,
) -> Result<&'a Path, CompileError> {
    Ok(context
        .dependencies()
        .get(module_path)
        .ok_or_else(|| CompileError::ModuleNotFound(module_path.into()))?
        .get(submodule_path)
        .ok_or_else(|| CompileError::ModuleNotFound(submodule_path.into()))?)
}

fn interpolate_variables(template: &str, variables: &ChainMap<String, String>) -> String {
    VARIABLE_PATTERN
        .replace_all(template, |captures: &Captures| {
            variables.get(&captures[1]).cloned().unwrap_or_default()
        })
        .replace("$$", "$")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast;
    use once_cell::sync::Lazy;
    use pretty_assertions::assert_eq;

    static ROOT_MODULE_PATH: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("build.ninja"));
    static DEFAULT_DEPENDENCIES: Lazy<ModuleDependencyMap> = Lazy::new(|| {
        [(ROOT_MODULE_PATH.clone(), Default::default())]
            .into_iter()
            .collect()
    });

    fn ast_explicit_build(
        outputs: Vec<String>,
        rule: impl Into<String>,
        inputs: Vec<String>,
        variable_definitions: Vec<ast::VariableDefinition>,
    ) -> ast::Build {
        ast::Build::new(
            outputs,
            vec![],
            rule,
            inputs,
            vec![],
            vec![],
            variable_definitions,
        )
    }

    fn ir_explicit_build(
        id: impl Into<String>,
        outputs: Vec<String>,
        rule: Rule,
        inputs: Vec<String>,
    ) -> Build {
        Build::new(id, outputs, vec![], rule.into(), inputs, vec![], None)
    }

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
            Configuration::new(Default::default(), Default::default(), None)
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
                        ast::Rule::new("foo", "$x", None).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(
                [(
                    "bar".into(),
                    ir_explicit_build("0", vec!["bar".into()], Rule::new("42", None), vec![])
                        .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                None
            )
        );
    }

    #[test]
    fn interpolate_two_variables_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "1").into(),
                        ast::VariableDefinition::new("y", "2").into(),
                        ast::Rule::new("foo", "$x $y", None).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(
                [(
                    "bar".into(),
                    ir_explicit_build("0", vec!["bar".into()], Rule::new("1 2", None), vec![])
                        .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                None
            )
        );
    }

    #[test]
    fn interpolate_variable_with_underscore_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x_y", "42").into(),
                        ast::Rule::new("foo", "$x_y", None).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(
                [(
                    "bar".into(),
                    ir_explicit_build("0", vec!["bar".into()], Rule::new("42", None), vec![])
                        .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                None
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
                        ast::Rule::new("foo", "$$", None).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(
                [(
                    "bar".into(),
                    ir_explicit_build("0", vec!["bar".into()], Rule::new("$", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                None
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
                        ast::Rule::new("foo", "$in", None).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec!["baz".into()], vec![])
                            .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        "0",
                        vec!["bar".into()],
                        Rule::new("baz", None),
                        vec!["baz".into()]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                None
            )
        );
    }

    #[test]
    fn interpolate_in_variable_with_implicit_input() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::Rule::new("foo", "$in", None).into(),
                        ast::Build::new(
                            vec!["bar".into()],
                            vec![],
                            "foo",
                            vec!["baz".into()],
                            vec!["blah".into()],
                            vec![],
                            vec![]
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
            Configuration::new(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        "0",
                        vec!["bar".into()],
                        Rule::new("baz", None),
                        vec!["baz".into(), "blah".into()]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                None
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
                        ast::Rule::new("foo", "$out", None).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(
                [(
                    "bar".into(),
                    ir_explicit_build("0", vec!["bar".into()], Rule::new("bar", None), vec![])
                        .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                None
            )
        );
    }

    #[test]
    fn interpolate_out_variable_with_implicit_output() {
        let build = Arc::new(Build::new(
            "0",
            vec!["bar".into()],
            vec!["baz".into()],
            Rule::new("bar", None).into(),
            vec![],
            vec![],
            None,
        ));

        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::Rule::new("foo", "$out", None).into(),
                        ast::Build::new(
                            vec!["bar".into()],
                            vec!["baz".into()],
                            "foo",
                            vec![],
                            vec![],
                            vec![],
                            vec![]
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
            Configuration::new(
                [("baz".into(), build.clone()), ("bar".into(), build)]
                    .into_iter()
                    .collect(),
                ["baz".into(), "bar".into()].into_iter().collect(),
                None
            )
        );
    }

    #[test]
    fn compile_order_only_inputs() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::Rule::new("foo", "$in", None).into(),
                        ast::Build::new(
                            vec!["bar".into()],
                            vec![],
                            "foo",
                            vec![],
                            vec![],
                            vec!["baz".into()],
                            vec![]
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
            Configuration::new(
                [(
                    "bar".into(),
                    Build::new(
                        "0",
                        vec!["bar".into()],
                        vec![],
                        Some(Rule::new("", None)),
                        vec![],
                        vec!["baz".into()],
                        None
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                None
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
                        ast::Rule::new("foo", "", None).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                        ast_explicit_build(vec!["baz".into()], "foo", vec![], vec![]).into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(
                [
                    (
                        "bar".into(),
                        ir_explicit_build("0", vec!["bar".into()], Rule::new("", None), vec![])
                            .into()
                    ),
                    (
                        "baz".into(),
                        ir_explicit_build("1", vec!["baz".into()], Rule::new("", None), vec![])
                            .into()
                    )
                ]
                .into_iter()
                .collect(),
                ["bar".into(), "baz".into()].into_iter().collect(),
                None
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
                        ast::Rule::new("foo", "$x", None).into(),
                        ast_explicit_build(
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
            Configuration::new(
                [(
                    "bar".into(),
                    ir_explicit_build("0", vec!["bar".into()], Rule::new("42", None), vec![])
                        .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                None
            )
        );
    }

    #[test]
    fn compile_phony_rule() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![ast_explicit_build(
                        vec!["foo".into()],
                        "phony",
                        vec!["bar".into()],
                        vec![]
                    )
                    .into(),])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(
                [(
                    "foo".into(),
                    Build::new(
                        "0",
                        vec!["foo".into()],
                        vec![],
                        None,
                        vec!["bar".into()],
                        vec![],
                        None
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["foo".into()].into_iter().collect(),
                None
            )
        );
    }

    #[test]
    fn compile_build_directory() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![ast::VariableDefinition::new("builddir", "foo").into()])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(Default::default(), Default::default(), Some("foo".into()))
        );
    }

    #[test]
    fn compile_dynamic_module_variable() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![ast_explicit_build(
                        vec!["foo".into()],
                        "phony",
                        vec![],
                        vec![ast::VariableDefinition::new("dyndep", "bar")]
                    )
                    .into()])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Configuration::new(
                [(
                    "foo".into(),
                    Build::new(
                        "0",
                        vec!["foo".into()],
                        vec![],
                        None,
                        vec![],
                        vec![],
                        Some("bar".into())
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["foo".into()].into_iter().collect(),
                None
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
                                ast::Rule::new("foo", "$x", None).into(),
                                ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![])
                                    .into()
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
                Configuration::new(
                    [(
                        "bar".into(),
                        ir_explicit_build("0", vec!["bar".into()], Rule::new("42", None), vec![])
                            .into()
                    )]
                    .into_iter()
                    .collect(),
                    ["bar".into()].into_iter().collect(),
                    None
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
                                ast::Rule::new("foo", "$x", None).into(),
                                ast::Submodule::new(SUBMODULE_PATH).into(),
                            ])
                        ),
                        (
                            SUBMODULE_PATH.into(),
                            ast::Module::new(vec![ast_explicit_build(
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
                Configuration::new(
                    [(
                        "bar".into(),
                        ir_explicit_build("0", vec!["bar".into()], Rule::new("42", None), vec![])
                            .into()
                    )]
                    .into_iter()
                    .collect(),
                    ["bar".into()].into_iter().collect(),
                    None
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
                                ast::Rule::new("foo", "$x", None).into(),
                                ast::Submodule::new(SUBMODULE_PATH).into(),
                                ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![])
                                    .into(),
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
                Configuration::new(
                    [(
                        "bar".into(),
                        ir_explicit_build("0", vec!["bar".into()], Rule::new("42", None), vec![])
                            .into()
                    )]
                    .into_iter()
                    .collect(),
                    ["bar".into()].into_iter().collect(),
                    None,
                )
            );
        }
    }
}

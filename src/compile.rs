mod chain_map;
mod context;
mod error;
mod global_state;
mod module_state;

pub use self::context::ModuleDependencyMap;
use self::{
    chain_map::ChainMap, context::Context, error::CompileError, global_state::GlobalState,
    module_state::ModuleState,
};
use crate::{
    ast,
    ir::{Build, Configuration, Rule},
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

static VARIABLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$([[:alpha:]][[:alnum:]]*)").unwrap());

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
                    context.generate_build_id(),
                    if build.rule() == PHONY_RULE {
                        None
                    } else {
                        let rule = &module_state
                            .rules
                            .get(build.rule())
                            .ok_or_else(|| CompileError::RuleNotFound(build.rule().into()))?;

                        Some(Rule::new(
                            interpolate_variables(rule.command(), &variables),
                            interpolate_variables(rule.description(), &variables),
                        ))
                    },
                    build
                        .inputs()
                        .iter()
                        .chain(build.implicit_inputs())
                        .cloned()
                        .collect(),
                    build.order_only_inputs().to_vec(),
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
    use crate::{ast, ir};
    use once_cell::sync::Lazy;
    use pretty_assertions::assert_eq;

    static ROOT_MODULE_PATH: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("build.ninja"));
    static DEFAULT_DEPENDENCIES: Lazy<ModuleDependencyMap> = Lazy::new(|| {
        [(ROOT_MODULE_PATH.clone(), Default::default())]
            .into_iter()
            .collect()
    });

    fn explicit_build(
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
            ir::Configuration::new(Default::default(), Default::default(), None)
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
                        explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
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
                    Build::new("0", Some(Rule::new("42", "")), vec![], vec![]).into()
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
                        ast::Rule::new("foo", "$x $y", "").into(),
                        explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
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
                    Build::new("0", Some(Rule::new("1 2", "")), vec![], vec![]).into()
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
                        ast::Rule::new("foo", "$$", "").into(),
                        explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into()
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
                    Build::new("0", Some(Rule::new("$", "")), vec![], vec![]).into()
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
                        ast::Rule::new("foo", "$in", "").into(),
                        explicit_build(vec!["bar".into()], "foo", vec!["baz".into()], vec![])
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
                    Build::new("0", Some(Rule::new("baz", "")), vec!["baz".into()], vec![]).into()
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
                        ast::Rule::new("foo", "$in", "").into(),
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
            ir::Configuration::new(
                [(
                    "bar".into(),
                    Build::new(
                        "0",
                        Some(Rule::new("baz", "")),
                        vec!["baz".into(), "blah".into()],
                        vec![]
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
                        ast::Rule::new("foo", "$out", "").into(),
                        explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
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
                    Build::new("0", Some(Rule::new("bar", "")), vec![], vec![]).into()
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
        let build = Arc::new(Build::new("0", Some(Rule::new("bar", "")), vec![], vec![]));

        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::Rule::new("foo", "$out", "").into(),
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
            ir::Configuration::new(
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
                        ast::Rule::new("foo", "$in", "").into(),
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
            ir::Configuration::new(
                [(
                    "bar".into(),
                    Build::new("0", Some(Rule::new("", "")), vec![], vec!["baz".into()]).into()
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
                        ast::Rule::new("foo", "", "").into(),
                        explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                        explicit_build(vec!["baz".into()], "foo", vec![], vec![]).into()
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
                    (
                        "bar".into(),
                        Build::new("0", Some(Rule::new("", "")), vec![], vec![]).into()
                    ),
                    (
                        "baz".into(),
                        Build::new("1", Some(Rule::new("", "")), vec![], vec![]).into()
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
                        ast::Rule::new("foo", "$x", "").into(),
                        explicit_build(
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
                [(
                    "bar".into(),
                    Build::new("0", Some(Rule::new("42", "")), vec![], vec![]).into()
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
                    ast::Module::new(vec![explicit_build(
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
            ir::Configuration::new(
                [(
                    "foo".into(),
                    Build::new("0", None, vec!["bar".into()], vec![]).into()
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
            ir::Configuration::new(Default::default(), Default::default(), Some("foo".into()))
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
                                explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into()
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
                    [(
                        "bar".into(),
                        Build::new("0", Some(Rule::new("42", "")), vec![], vec![]).into()
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
                                ast::Rule::new("foo", "$x", "").into(),
                                ast::Submodule::new(SUBMODULE_PATH).into(),
                            ])
                        ),
                        (
                            SUBMODULE_PATH.into(),
                            ast::Module::new(vec![explicit_build(
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
                    [(
                        "bar".into(),
                        Build::new("0", Some(Rule::new("42", "")), vec![], vec![]).into()
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
                                ast::Rule::new("foo", "$x", "").into(),
                                ast::Submodule::new(SUBMODULE_PATH).into(),
                                explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
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
                    [(
                        "bar".into(),
                        Build::new("0", Some(Rule::new("42", "")), vec![], vec![]).into()
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

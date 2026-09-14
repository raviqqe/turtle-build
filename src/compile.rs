mod context;
mod error;
mod global_state;
mod module_state;

pub use self::error::CompileError;
use self::{context::Context, global_state::GlobalState, module_state::ModuleState};
use crate::{
    ast,
    ir::{Build, Config, DynamicBuild, DynamicConfig, HeaderDependency, Rule},
    module_dependency::ModuleDependencyMap,
};
use alloc::{borrow::Cow, sync::Arc};
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use train_map::TrainMap;

const PHONY_RULE: &str = "phony";
const BUILD_DIRECTORY_VARIABLE: &str = "builddir";
const COMMAND_VARIABLE: &str = "command";
const DESCRIPTION_VARIABLE: &str = "description";
const DEPFILE_VARIABLE: &str = "depfile";
const DEPS_VARIABLE: &str = "deps";
const DYNAMIC_MODULE_VARIABLE: &str = "dyndep";
const SOURCE_VARIABLE_NAME: &str = "srcdep";
const MSVC_DEPS_PREFIX_VARIABLE: &str = "msvc_deps_prefix";
// Matches the default prefix of `cl.exe`'s own `/showIncludes` output.
const DEFAULT_MSVC_DEPS_PREFIX: &str = "Note: including file: ";

static VARIABLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$(\$|\{([[:alnum:]_.-]+)\}|[[:alpha:]_][[:alnum:]_]*)").unwrap());

/// Compiles modules.
// TODO Use a string pool for paths.
pub fn compile(
    modules: &HashMap<PathBuf, ast::Module>,
    dependencies: &ModuleDependencyMap,
    root_module_path: &Path,
) -> Result<Config, CompileError> {
    let context = Context::new(modules, dependencies);

    let mut global_state = GlobalState {
        outputs: Default::default(),
        default_outputs: Default::default(),
        source_map: Default::default(),
    };
    let mut module_state = ModuleState {
        rules: TrainMap::new(),
        variables: TrainMap::new(),
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

    Ok(Config::new(
        global_state.outputs,
        default_outputs,
        global_state.source_map,
        module_state
            .variables
            .get(BUILD_DIRECTORY_VARIABLE)
            .cloned(),
    ))
}

fn compile_module<'a>(
    context: &'a Context,
    global_state: &mut GlobalState,
    module_state: &mut ModuleState<'a, '_>,
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

                if build.rule() != PHONY_RULE {
                    variables.extend(
                        module_state
                            .rules
                            .get(build.rule())
                            .ok_or_else(|| CompileError::RuleNotFound(build.rule().into()))?
                            .iter()
                            .cloned(),
                    );
                }

                variables.extend(
                    build
                        .variable_definitions()
                        .iter()
                        .map(|definition| (definition.name(), definition.value().into())),
                );

                let outputs = interpolate_paths(build.outputs(), &variables);
                let implicit_outputs = interpolate_paths(build.implicit_outputs(), &variables);
                let inputs = interpolate_paths(build.inputs(), &variables);
                let implicit_inputs = interpolate_paths(build.implicit_inputs(), &variables);
                let order_only_inputs = interpolate_paths(build.order_only_inputs(), &variables);

                variables.extend([
                    ("in", inputs.join(" ").into()),
                    ("out", outputs.join(" ").into()),
                ]);

                let ir = Arc::new(Build::new(
                    outputs,
                    implicit_outputs,
                    if build.rule() == PHONY_RULE {
                        None
                    } else {
                        Some(
                            Rule::new(
                                resolve_variable(COMMAND_VARIABLE, &variables).unwrap_or_default(),
                                resolve_variable(DESCRIPTION_VARIABLE, &variables),
                            )
                            .with_header_dependency(
                                compile_header_dependency(build.rule(), &variables)?,
                            ),
                        )
                    },
                    inputs.into_iter().chain(implicit_inputs).collect(),
                    order_only_inputs,
                    resolve_variable(DYNAMIC_MODULE_VARIABLE, &variables).map(Into::into),
                ));

                let outputs = || ir.outputs().iter().chain(ir.implicit_outputs());

                global_state
                    .outputs
                    .extend(outputs().map(|output| (output.clone(), ir.clone())));

                if let Some(source) = variables.get(SOURCE_VARIABLE_NAME) {
                    global_state
                        .source_map
                        .extend(outputs().map(|output| (output.clone(), source.clone())));
                }
            }
            ast::Statement::Default(default) => {
                global_state.default_outputs.extend(interpolate_paths(
                    default.outputs(),
                    &module_state.variables,
                ));
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
                module_state.rules.insert(
                    rule.name(),
                    rule.variable_definitions()
                        .iter()
                        .map(|definition| (definition.name(), definition.value().into()))
                        .collect(),
                );
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
                module_state.variables.insert(
                    definition.name(),
                    interpolate_variables(definition.value(), &module_state.variables).into(),
                );
            }
        }
    }

    Ok(())
}

fn compile_header_dependency(
    rule: &str,
    variables: &TrainMap<&str, Arc<str>>,
) -> Result<Option<HeaderDependency>, CompileError> {
    Ok(
        match (
            resolve_variable(DEPS_VARIABLE, variables).as_deref(),
            resolve_variable(DEPFILE_VARIABLE, variables),
        ) {
            (None, None) => None,
            (None, Some(path)) => Some(HeaderDependency::Make { path }),
            (Some("gcc"), Some(path)) => Some(HeaderDependency::Gcc { path }),
            (Some("gcc"), None) => return Err(CompileError::MissingDepfile(rule.into())),
            (Some("msvc"), _) => Some(HeaderDependency::Msvc {
                prefix: resolve_variable(MSVC_DEPS_PREFIX_VARIABLE, variables)
                    .unwrap_or_else(|| DEFAULT_MSVC_DEPS_PREFIX.into()),
            }),
            (Some(deps), _) => return Err(CompileError::InvalidDependencyStyle(deps.into())),
        },
    )
}

pub fn compile_dynamic(module: &ast::DynamicModule) -> Result<DynamicConfig, CompileError> {
    Ok(DynamicConfig::new(
        module
            .builds()
            .iter()
            .map(|build| {
                (
                    build.output().into(),
                    DynamicBuild::new(
                        build
                            .implicit_inputs()
                            .iter()
                            .map(|string| string.as_str().into())
                            .collect(),
                    ),
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

fn resolve_variable(name: &str, variables: &TrainMap<&str, Arc<str>>) -> Option<String> {
    variables
        .get(name)
        .map(|value| interpolate_variables(value, variables).into_owned())
        .filter(|value| !value.is_empty())
}

fn interpolate_paths(paths: &[String], variables: &TrainMap<&str, Arc<str>>) -> Vec<Arc<str>> {
    paths
        .iter()
        .map(|path| interpolate_variables(path, variables).into())
        .collect()
}

fn interpolate_variables<'a>(
    template: &'a str,
    variables: &TrainMap<&str, Arc<str>>,
) -> Cow<'a, str> {
    VARIABLE_PATTERN.replace_all(template, |captures: &Captures| match &captures[1] {
        "$" => "$",
        name => variables
            .get(captures.get(2).map_or(name, |name| name.as_str()))
            .map(|string| string.as_ref())
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast;
    use once_cell::sync::Lazy;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

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

    fn ast_rule(name: &str, variable_definitions: &[(&str, &str)]) -> ast::Rule {
        ast::Rule::new(
            name,
            variable_definitions
                .iter()
                .map(|(name, value)| ast::VariableDefinition::new(*name, *value))
                .collect(),
        )
    }

    fn ir_explicit_build(outputs: Vec<Arc<str>>, rule: Rule, inputs: Vec<Arc<str>>) -> Build {
        Build::new(outputs, vec![], rule.into(), inputs, vec![], None)
    }

    fn create_simple_config(
        outputs: HashMap<Arc<str>, Arc<Build>>,
        default_outputs: HashSet<Arc<str>>,
    ) -> Config {
        Config::new(outputs, default_outputs, Default::default(), None)
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
            create_simple_config(Default::default(), Default::default())
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
                        ast_rule("foo", &[("command", "$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
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
                        ast_rule("foo", &[("command", "$x $y")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("1 2", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
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
                        ast_rule("foo", &[("command", "$x_y")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_variable_with_braces_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x.y", "42").into(),
                        ast_rule("foo", &[("command", "${x.y}")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42", None), vec![]).into()
                )]
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
                        ast_rule("foo", &[("command", "$$")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("$", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_escaped_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "42").into(),
                        ast_rule("foo", &[("command", "$$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("$x", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_nested_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "42").into(),
                        ast::VariableDefinition::new("y", "$x").into(),
                        ast_rule("foo", &[("command", "$y")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn expand_variable_on_definition() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "1").into(),
                        ast::VariableDefinition::new("y", "$x").into(),
                        ast::VariableDefinition::new("x", "2").into(),
                        ast_rule("foo", &[("command", "$y")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("1", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn append_value_to_variable() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "1").into(),
                        ast::VariableDefinition::new("x", "$x 2").into(),
                        ast_rule("foo", &[("command", "$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("1 2", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn expand_escaped_variable_on_definition() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("y", "42").into(),
                        ast::VariableDefinition::new("x", "$$y").into(),
                        ast_rule("foo", &[("command", "$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("$y", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn unescape_dollar_signs_once() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "$$$$").into(),
                        ast_rule("foo", &[("command", "$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("$$", None), vec![]).into()
                )]
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
                        ast_rule("foo", &[("command", "$in")]).into(),
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
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("baz", None),
                        vec!["baz".into()]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
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
                        ast_rule("foo", &[("command", "$in")]).into(),
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
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("baz", None),
                        vec!["baz".into(), "blah".into()]
                    )
                    .into()
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
                        ast_rule("foo", &[("command", "$out")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("bar", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_out_variable_with_implicit_output() {
        let build = Arc::new(Build::new(
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
                        ast_rule("foo", &[("command", "$out")]).into(),
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
            create_simple_config(
                [("baz".into(), build.clone()), ("bar".into(), build)]
                    .into_iter()
                    .collect(),
                ["baz".into(), "bar".into()].into_iter().collect()
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
                        ast_rule("foo", &[("command", "$in")]).into(),
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
            create_simple_config(
                [(
                    "bar".into(),
                    Build::new(
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
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_variables_in_paths() {
        let build = Arc::new(Build::new(
            vec!["foo/output".into()],
            vec!["foo/implicit_output".into()],
            Rule::new("foo/input foo/output", None).into(),
            vec!["foo/input".into(), "foo/implicit_input".into()],
            vec!["foo/order_only_input".into()],
            None,
        ));

        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "foo").into(),
                        ast_rule("bar", &[("command", "$in $out")]).into(),
                        ast::Build::new(
                            vec!["$x/output".into()],
                            vec!["${x}/implicit_output".into()],
                            "bar",
                            vec!["$x/input".into()],
                            vec!["${x}/implicit_input".into()],
                            vec!["$x/order_only_input".into()],
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
            create_simple_config(
                [
                    ("foo/output".into(), build.clone()),
                    ("foo/implicit_output".into(), build)
                ]
                .into_iter()
                .collect(),
                ["foo/output".into(), "foo/implicit_output".into()]
                    .into_iter()
                    .collect()
            )
        );
    }

    #[test]
    fn interpolate_build_local_variable_in_path() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "$out")]).into(),
                        ast_explicit_build(
                            vec!["$x/bar".into()],
                            "foo",
                            vec![],
                            vec![ast::VariableDefinition::new("x", "baz")]
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
            create_simple_config(
                [(
                    "baz/bar".into(),
                    ir_explicit_build(vec!["baz/bar".into()], Rule::new("baz/bar", None), vec![])
                        .into()
                )]
                .into_iter()
                .collect(),
                ["baz/bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn unescape_dollar_sign_in_path() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "")]).into(),
                        ast_explicit_build(vec!["bar$$baz".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar$baz".into(),
                    ir_explicit_build(vec!["bar$baz".into()], Rule::new("", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar$baz".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn do_not_interpolate_in_and_out_variables_in_paths() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "$in $out")]).into(),
                        ast_explicit_build(
                            vec!["bar$out".into()],
                            "foo",
                            vec!["baz$in".into()],
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
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("baz bar", None),
                        vec!["baz".into()]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_variable_in_default_output() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "foo").into(),
                        ast_rule("bar", &[("command", "")]).into(),
                        ast_explicit_build(vec!["foo/baz".into()], "bar", vec![], vec![]).into(),
                        ast_explicit_build(vec!["qux".into()], "bar", vec![], vec![]).into(),
                        ast::DefaultOutput::new(vec!["$x/baz".into()]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [
                    (
                        "foo/baz".into(),
                        ir_explicit_build(vec!["foo/baz".into()], Rule::new("", None), vec![])
                            .into()
                    ),
                    (
                        "qux".into(),
                        ir_explicit_build(vec!["qux".into()], Rule::new("", None), vec![]).into()
                    )
                ]
                .into_iter()
                .collect(),
                ["foo/baz".into()].into_iter().collect()
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
                        ast_rule("foo", &[("command", "")]).into(),
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
            create_simple_config(
                [
                    (
                        "bar".into(),
                        ir_explicit_build(vec!["bar".into()], Rule::new("", None), vec![]).into()
                    ),
                    (
                        "baz".into(),
                        ir_explicit_build(vec!["baz".into()], Rule::new("", None), vec![]).into()
                    )
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
                        ast_rule("foo", &[("command", "$x")]).into(),
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
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_rule_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule(
                            "foo",
                            &[("command", "$description"), ("description", "bar")]
                        )
                        .into(),
                        ast_explicit_build(vec!["baz".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "baz".into(),
                    ir_explicit_build(
                        vec!["baz".into()],
                        Rule::new("bar", Some("bar".into())),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["baz".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_last_command_in_rule() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "first"), ("command", "second")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("second", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_build_level_command_shadowing_rule_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "rule")]).into(),
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo",
                            vec![],
                            vec![ast::VariableDefinition::new("command", "build")]
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
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("build", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_global_description() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("description", "global").into(),
                        ast_rule("foo", &[("command", "")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("", Some("global".into())),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_source_map() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "foo")]).into(),
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo",
                            vec![],
                            vec![ast::VariableDefinition::new(
                                SOURCE_VARIABLE_NAME,
                                "oh-my-src"
                            )]
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
            Config::new(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("foo", None), vec![]).into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                [("bar".into(), "oh-my-src".into())].into_iter().collect(),
                None,
            )
        );
    }

    #[test]
    fn compile_phony_rule() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_explicit_build(vec!["foo".into()], "phony", vec!["bar".into()], vec![])
                            .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "foo".into(),
                    Build::new(
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
                ["foo".into()].into_iter().collect()
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
            Config::new(
                Default::default(),
                Default::default(),
                Default::default(),
                Some("foo".into())
            )
        );
    }

    #[test]
    fn compile_build_directory_with_variable() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x", "foo").into(),
                        ast::VariableDefinition::new("builddir", "$x/bar").into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            Config::new(
                Default::default(),
                Default::default(),
                Default::default(),
                Some("foo/bar".into())
            )
        );
    }

    #[test]
    fn compile_dynamic_module_variable() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_explicit_build(
                            vec!["foo".into()],
                            "phony",
                            vec![],
                            vec![ast::VariableDefinition::new("dyndep", "bar")]
                        )
                        .into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "foo".into(),
                    Build::new(
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
                ["foo".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_rule_level_dynamic_module_variable() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", ""), ("dyndep", "$out.dd")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    Build::new(
                        vec!["bar".into()],
                        vec![],
                        Some(Rule::new("", None)),
                        vec![],
                        vec![],
                        Some("bar.dd".into())
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_build_level_dynamic_module_variable_shadowing_rule_level_one() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", ""), ("dyndep", "rule.dd")]).into(),
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo",
                            vec![],
                            vec![ast::VariableDefinition::new("dyndep", "build.dd")]
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
            create_simple_config(
                [(
                    "bar".into(),
                    Build::new(
                        vec!["bar".into()],
                        vec![],
                        Some(Rule::new("", None)),
                        vec![],
                        vec![],
                        Some("build.dd".into())
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_make_header_dependency() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "bar"), ("depfile", "foo.d")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Make {
                                path: "foo.d".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_gcc_header_dependency() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule(
                            "foo",
                            &[("command", "bar"), ("depfile", "foo.d"), ("deps", "gcc")]
                        )
                        .into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Gcc {
                                path: "foo.d".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_make_header_dependency_with_build_level_depfile() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "bar")]).into(),
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo",
                            vec![],
                            vec![ast::VariableDefinition::new("depfile", "foo.d")]
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
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Make {
                                path: "foo.d".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_gcc_header_dependency_with_build_level_deps() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "bar"), ("depfile", "$out.d")]).into(),
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo",
                            vec![],
                            vec![ast::VariableDefinition::new("deps", "gcc")]
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
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Gcc {
                                path: "bar.d".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_make_header_dependency_with_build_level_empty_deps_shadowing_rule_deps() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule(
                            "foo",
                            &[("command", "bar"), ("depfile", "foo.d"), ("deps", "gcc")]
                        )
                        .into(),
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo",
                            vec![],
                            vec![ast::VariableDefinition::new("deps", "")]
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
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Make {
                                path: "foo.d".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn fail_to_compile_gcc_header_dependency_without_depfile() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "bar"), ("deps", "gcc")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            ),
            Err(CompileError::MissingDepfile("foo".into()))
        );
    }

    #[test]
    fn fail_to_compile_unknown_header_dependency() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "bar"), ("deps", "clang")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            ),
            Err(CompileError::InvalidDependencyStyle("clang".into()))
        );
    }

    #[test]
    fn compile_msvc_header_dependency_with_default_prefix() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "bar"), ("deps", "msvc")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Msvc {
                                prefix: "Note: including file: ".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_msvc_header_dependency_ignoring_depfile() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule(
                            "foo",
                            &[("command", "bar"), ("depfile", "foo.d"), ("deps", "msvc")]
                        )
                        .into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Msvc {
                                prefix: "Note: including file: ".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_msvc_header_dependency_with_empty_prefix() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule(
                            "foo",
                            &[
                                ("command", "bar"),
                                ("deps", "msvc"),
                                ("msvc_deps_prefix", "")
                            ]
                        )
                        .into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Msvc {
                                prefix: "Note: including file: ".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_msvc_header_dependency_with_custom_prefix() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule(
                            "foo",
                            &[
                                ("command", "bar"),
                                ("deps", "msvc"),
                                ("msvc_deps_prefix", "Hinweis: Einlesen der Datei ")
                            ]
                        )
                        .into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Msvc {
                                prefix: "Hinweis: Einlesen der Datei ".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_msvc_header_dependency_with_global_prefix() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new(
                            "msvc_deps_prefix",
                            "Hinweis: Einlesen der Datei "
                        )
                        .into(),
                        ast_rule("foo", &[("command", "bar"), ("deps", "msvc")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Msvc {
                                prefix: "Hinweis: Einlesen der Datei ".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_msvc_header_dependency_with_rule_prefix_shadowing_global_prefix() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("msvc_deps_prefix", "global prefix: ").into(),
                        ast_rule(
                            "foo",
                            &[
                                ("command", "bar"),
                                ("deps", "msvc"),
                                ("msvc_deps_prefix", "rule prefix: ")
                            ]
                        )
                        .into(),
                        ast_explicit_build(vec!["bar".into()], "foo", vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Msvc {
                                prefix: "rule prefix: ".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn compile_msvc_header_dependency_with_build_level_prefix_shadowing_rule_prefix() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule(
                            "foo",
                            &[
                                ("command", "bar"),
                                ("deps", "msvc"),
                                ("msvc_deps_prefix", "rule prefix: ")
                            ]
                        )
                        .into(),
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo",
                            vec![],
                            vec![ast::VariableDefinition::new(
                                "msvc_deps_prefix",
                                "build prefix: "
                            )]
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
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar", None).with_header_dependency(Some(
                            HeaderDependency::Msvc {
                                prefix: "build prefix: ".into()
                            }
                        )),
                        vec![]
                    )
                    .into()
                )]
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
                                ast_rule("foo", &[("command", "$x")]).into(),
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
                create_simple_config(
                    [(
                        "bar".into(),
                        ir_explicit_build(vec!["bar".into()], Rule::new("42", None), vec![]).into()
                    )]
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
                                ast_rule("foo", &[("command", "$x")]).into(),
                                ast::Submodule::new(SUBMODULE_PATH).into(),
                            ])
                        ),
                        (
                            SUBMODULE_PATH.into(),
                            ast::Module::new(vec![
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
                create_simple_config(
                    [(
                        "bar".into(),
                        ir_explicit_build(vec!["bar".into()], Rule::new("42", None), vec![]).into()
                    )]
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
                                ast_rule("foo", &[("command", "$x")]).into(),
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
                create_simple_config(
                    [(
                        "bar".into(),
                        ir_explicit_build(vec!["bar".into()], Rule::new("42", None), vec![]).into()
                    )]
                    .into_iter()
                    .collect(),
                    ["bar".into()].into_iter().collect()
                )
            );
        }
    }
}

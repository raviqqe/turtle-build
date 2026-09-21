mod context;
mod error;
mod global_state;
mod module_state;

pub use self::error::CompileError;
use self::{context::CompileContext, global_state::GlobalState, module_state::ModuleState};
use crate::{
    ast,
    ir::{Build, Config, DynamicBuild, DynamicConfig, HeaderDependency, Pool, Rule},
    module_dependency::ModuleDependencyMap,
    path_pool::PathPool,
};
use alloc::{borrow::Cow, sync::Arc};
use core::num::NonZeroUsize;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use std::{
    collections::{HashMap, hash_map::Entry},
    path::{Path, PathBuf},
};
use train_map::TrainMap;

const PHONY_RULE: &str = "phony";
const CONSOLE_POOL: &str = "console";
const BUILD_DIRECTORY_VARIABLE: &str = "builddir";
const COMMAND_VARIABLE: &str = "command";
const DESCRIPTION_VARIABLE: &str = "description";
const DEPFILE_VARIABLE: &str = "depfile";
const DEPS_VARIABLE: &str = "deps";
const DYNAMIC_MODULE_VARIABLE: &str = "dyndep";
const POOL_VARIABLE: &str = "pool";
const SOURCE_VARIABLE_NAME: &str = "srcdep";
const MSVC_DEPS_PREFIX_VARIABLE: &str = "msvc_deps_prefix";
// Matches the default prefix of `cl.exe`'s own `/showIncludes` output.
const DEFAULT_MSVC_DEPS_PREFIX: &str = "Note: including file: ";

static VARIABLE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$([$ :]|\{([[:alnum:]_.-]+)\}|[[:alpha:]_][[:alnum:]_]*)").unwrap());

/// Compiles modules.
pub fn compile(
    modules: &HashMap<PathBuf, ast::Module>,
    dependencies: &ModuleDependencyMap,
    root_module_path: &Path,
    path_pool: &PathPool,
) -> Result<Config, CompileError> {
    let context = CompileContext::new(modules, dependencies, path_pool);

    let mut global_state = GlobalState {
        outputs: Default::default(),
        default_outputs: Default::default(),
        source_map: Default::default(),
        pools: [(CONSOLE_POOL.into(), None)].into(),
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
        global_state
            .pools
            .into_iter()
            .filter_map(|(name, depth)| Some((name, depth?)))
            .collect(),
        module_state
            .variables
            .get(BUILD_DIRECTORY_VARIABLE)
            .cloned(),
    ))
}

fn compile_module<'a>(
    context: &'a CompileContext,
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

                let pool = compile_pool(&global_state.pools, &variables)?;

                let outputs = interpolate_strings(context, build.outputs(), &variables);
                let implicit_outputs =
                    interpolate_strings(context, build.implicit_outputs(), &variables);
                let inputs = interpolate_strings(context, build.inputs(), &variables);
                let implicit_inputs =
                    interpolate_strings(context, build.implicit_inputs(), &variables);
                let order_only_inputs =
                    interpolate_strings(context, build.order_only_inputs(), &variables);

                variables.extend([
                    ("in", inputs.join(" ").into()),
                    ("in_newline", inputs.join("\n").into()),
                    ("out", outputs.join(" ").into()),
                ]);

                let dynamic_module = compile_dynamic_module_path(
                    inputs
                        .iter()
                        .chain(&implicit_inputs)
                        .chain(&order_only_inputs),
                    &variables,
                )?;

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
                            .with_pool(pool)
                            .with_header_dependency(
                                compile_header_dependency(context, build.rule(), &variables)?,
                            ),
                        )
                    },
                    inputs.into_iter().chain(implicit_inputs).collect(),
                    order_only_inputs,
                    dynamic_module,
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
                global_state.default_outputs.extend(interpolate_strings(
                    context,
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
            ast::Statement::Pool(pool) => {
                let Entry::Vacant(entry) = global_state.pools.entry(pool.name().into()) else {
                    return Err(CompileError::DuplicatePool(pool.name().into()));
                };
                let depth = interpolate_variables(pool.depth(), &module_state.variables);

                entry.insert(NonZeroUsize::new(depth.trim().parse().map_err(|_| {
                    CompileError::InvalidPoolDepth(pool.name().into(), depth.as_ref().into())
                })?));
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
    context: &CompileContext,
    rule: &str,
    variables: &TrainMap<&str, Arc<str>>,
) -> Result<Option<HeaderDependency>, CompileError> {
    Ok(
        match (
            resolve_variable(DEPS_VARIABLE, variables).as_deref(),
            resolve_variable(DEPFILE_VARIABLE, variables),
        ) {
            (None, None) => None,
            (None, Some(path)) => Some(HeaderDependency::Make {
                path: context.path_pool().intern(&path),
            }),
            (Some("gcc"), Some(path)) => Some(HeaderDependency::Gcc {
                path: context.path_pool().intern(&path),
            }),
            (Some("gcc"), None) => return Err(CompileError::MissingDepfile(rule.into())),
            (Some("msvc"), _) => Some(HeaderDependency::Msvc {
                prefix: resolve_variable(MSVC_DEPS_PREFIX_VARIABLE, variables)
                    .unwrap_or_else(|| DEFAULT_MSVC_DEPS_PREFIX.into()),
            }),
            (Some(deps), _) => return Err(CompileError::InvalidDependencyStyle(deps.into())),
        },
    )
}

fn compile_pool(
    pools: &HashMap<Arc<str>, Option<NonZeroUsize>>,
    variables: &TrainMap<&str, Arc<str>>,
) -> Result<Option<Pool>, CompileError> {
    let Some(name) = resolve_variable(POOL_VARIABLE, variables) else {
        return Ok(None);
    };
    let (name, depth) = pools
        .get_key_value(name.as_str())
        .ok_or(CompileError::PoolNotFound(name))?;

    Ok(if name.as_ref() == CONSOLE_POOL {
        Some(Pool::Console)
    } else if depth.is_some() {
        Some(Pool::Limited(name.clone()))
    } else {
        None
    })
}

fn compile_dynamic_module_path<'a>(
    mut inputs: impl Iterator<Item = &'a Arc<str>>,
    variables: &TrainMap<&str, Arc<str>>,
) -> Result<Option<Arc<str>>, CompileError> {
    let Some(path) = resolve_variable(DYNAMIC_MODULE_VARIABLE, variables) else {
        return Ok(None);
    };

    Ok(Some(
        inputs
            .find(|input| input.as_ref() == path)
            .ok_or(CompileError::DynamicModuleNotInput(path))?
            .clone(),
    ))
}

pub fn compile_dynamic(
    module: &ast::DynamicModule,
    path_pool: &PathPool,
) -> Result<DynamicConfig, CompileError> {
    let intern = |path: &str| path_pool.intern(&interpolate_variables(path, &TrainMap::new()));

    Ok(DynamicConfig::new(
        module
            .builds()
            .iter()
            .map(|build| {
                (
                    intern(build.output()),
                    DynamicBuild::new(
                        build
                            .implicit_inputs()
                            .iter()
                            .map(|path| intern(path))
                            .collect(),
                    ),
                )
            })
            .collect(),
    ))
}

fn resolve_dependency<'a>(
    context: &'a CompileContext,
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

fn interpolate_strings(
    context: &CompileContext,
    strings: &[String],
    variables: &TrainMap<&str, Arc<str>>,
) -> Vec<Arc<str>> {
    strings
        .iter()
        .map(|string| {
            context
                .path_pool()
                .intern(&interpolate_variables(string, variables))
        })
        .collect()
}

fn interpolate_variables<'a>(
    template: &'a str,
    variables: &TrainMap<&str, Arc<str>>,
) -> Cow<'a, str> {
    VARIABLE_PATTERN.replace_all(template, |captures: &Captures| match &captures[1] {
        "$" => "$",
        " " => " ",
        ":" => ":",
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
        rule: String,
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
            name.into(),
            variable_definitions
                .iter()
                .map(|&(name, value)| ast::VariableDefinition::new(name.into(), value.into()))
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
        Config::new(
            outputs,
            default_outputs,
            Default::default(),
            Default::default(),
            None,
        )
    }

    fn compile_root_module(statements: Vec<ast::Statement>) -> Result<Config, CompileError> {
        compile(
            &[(ROOT_MODULE_PATH.clone(), ast::Module::new(statements))]
                .into_iter()
                .collect(),
            &DEFAULT_DEPENDENCIES,
            &ROOT_MODULE_PATH,
            &Default::default(),
        )
    }

    fn create_pool_config(pool: Option<Pool>, pools: &[(&str, usize)]) -> Config {
        Config::new(
            [(
                "bar".into(),
                ir_explicit_build(
                    vec!["bar".into()],
                    Rule::new("baz".into(), None).with_pool(pool),
                    vec![],
                )
                .into(),
            )]
            .into_iter()
            .collect(),
            ["bar".into()].into_iter().collect(),
            Default::default(),
            pools
                .iter()
                .map(|&(name, depth)| (name.into(), NonZeroUsize::new(depth).unwrap()))
                .collect(),
            None,
        )
    }

    fn limited_pool(name: &str) -> Option<Pool> {
        Some(Pool::Limited(name.into()))
    }

    #[test]
    fn compile_empty_module() {
        assert_eq!(
            compile(
                &[(ROOT_MODULE_PATH.clone(), ast::Module::new(vec![]))]
                    .into_iter()
                    .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
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
                        ast::VariableDefinition::new("x".into(), "42".into()).into(),
                        ast_rule("foo", &[("command", "$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42".into(), None), vec![])
                        .into()
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
                        ast::VariableDefinition::new("x".into(), "1".into()).into(),
                        ast::VariableDefinition::new("y".into(), "2".into()).into(),
                        ast_rule("foo", &[("command", "$x $y")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("1 2".into(), None), vec![])
                        .into()
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
                        ast::VariableDefinition::new("x_y".into(), "42".into()).into(),
                        ast_rule("foo", &[("command", "$x_y")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42".into(), None), vec![])
                        .into()
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
                        ast::VariableDefinition::new("x.y".into(), "42".into()).into(),
                        ast_rule("foo", &[("command", "${x.y}")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42".into(), None), vec![])
                        .into()
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
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("$".into(), None), vec![])
                        .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn interpolate_space_and_colon_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "C$:\\foo$ bar")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("C:\\foo bar".into(), None),
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
    fn interpolate_escaped_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x".into(), "42".into()).into(),
                        ast_rule("foo", &[("command", "$$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("$x".into(), None), vec![])
                        .into()
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
                        ast::VariableDefinition::new("x".into(), "42".into()).into(),
                        ast::VariableDefinition::new("y".into(), "$x".into()).into(),
                        ast_rule("foo", &[("command", "$y")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42".into(), None), vec![])
                        .into()
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
                        ast::VariableDefinition::new("x".into(), "1".into()).into(),
                        ast::VariableDefinition::new("y".into(), "$x".into()).into(),
                        ast::VariableDefinition::new("x".into(), "2".into()).into(),
                        ast_rule("foo", &[("command", "$y")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("1".into(), None), vec![])
                        .into()
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
                        ast::VariableDefinition::new("x".into(), "1".into()).into(),
                        ast::VariableDefinition::new("x".into(), "$x 2".into()).into(),
                        ast_rule("foo", &[("command", "$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("1 2".into(), None), vec![])
                        .into()
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
                        ast::VariableDefinition::new("y".into(), "42".into()).into(),
                        ast::VariableDefinition::new("x".into(), "$$y".into()).into(),
                        ast_rule("foo", &[("command", "$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("$y".into(), None), vec![])
                        .into()
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
                        ast::VariableDefinition::new("x".into(), "$$$$".into()).into(),
                        ast_rule("foo", &[("command", "$x")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("$$".into(), None), vec![])
                        .into()
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
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo".into(),
                            vec!["baz".into()],
                            vec![]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("baz".into(), None),
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
                            "foo".into(),
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
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("baz".into(), None),
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
    fn interpolate_in_newline_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "$in_newline")]).into(),
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo".into(),
                            vec!["baz".into(), "qux".into()],
                            vec![]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("baz\nqux".into(), None),
                        vec!["baz".into(), "qux".into()]
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
    fn interpolate_in_newline_variable_with_implicit_and_order_only_inputs() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "$in_newline")]).into(),
                        ast::Build::new(
                            vec!["bar".into()],
                            vec![],
                            "foo".into(),
                            vec!["baz".into(), "qux".into()],
                            vec!["blah".into()],
                            vec!["corge".into()],
                            vec![]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    Build::new(
                        vec!["bar".into()],
                        vec![],
                        Some(Rule::new("baz\nqux".into(), None)),
                        vec!["baz".into(), "qux".into(), "blah".into()],
                        vec!["corge".into()],
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
    fn interpolate_out_variable_in_command() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "$out")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("bar".into(), None), vec![])
                        .into()
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
            Rule::new("bar".into(), None).into(),
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
                            "foo".into(),
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
                &ROOT_MODULE_PATH,
                &Default::default(),
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
                            "foo".into(),
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
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    Build::new(
                        vec!["bar".into()],
                        vec![],
                        Some(Rule::new("".into(), None)),
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
            Rule::new("foo/input foo/output".into(), None).into(),
            vec!["foo/input".into(), "foo/implicit_input".into()],
            vec!["foo/order_only_input".into()],
            None,
        ));

        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("x".into(), "foo".into()).into(),
                        ast_rule("bar", &[("command", "$in $out")]).into(),
                        ast::Build::new(
                            vec!["$x/output".into()],
                            vec!["${x}/implicit_output".into()],
                            "bar".into(),
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
                &ROOT_MODULE_PATH,
                &Default::default(),
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
                            "foo".into(),
                            vec![],
                            vec![ast::VariableDefinition::new("x".into(), "baz".into())]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "baz/bar".into(),
                    ir_explicit_build(
                        vec!["baz/bar".into()],
                        Rule::new("baz/bar".into(), None),
                        vec![]
                    )
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
                        ast_explicit_build(vec!["bar$$baz".into()], "foo".into(), vec![], vec![])
                            .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar$baz".into(),
                    ir_explicit_build(vec!["bar$baz".into()], Rule::new("".into(), None), vec![])
                        .into()
                )]
                .into_iter()
                .collect(),
                ["bar$baz".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn unescape_space_and_colon_in_path() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "$in $out")]).into(),
                        ast_explicit_build(
                            vec!["C$:\\bar$ baz".into()],
                            "foo".into(),
                            vec!["C$:\\qux".into()],
                            vec![]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "C:\\bar baz".into(),
                    ir_explicit_build(
                        vec!["C:\\bar baz".into()],
                        Rule::new("C:\\qux C:\\bar baz".into(), None),
                        vec!["C:\\qux".into()]
                    )
                    .into()
                )]
                .into_iter()
                .collect(),
                ["C:\\bar baz".into()].into_iter().collect()
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
                            "foo".into(),
                            vec!["baz$in".into()],
                            vec![]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("baz bar".into(), None),
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
                        ast::VariableDefinition::new("x".into(), "foo".into()).into(),
                        ast_rule("bar", &[("command", "")]).into(),
                        ast_explicit_build(vec!["foo/baz".into()], "bar".into(), vec![], vec![])
                            .into(),
                        ast_explicit_build(vec!["qux".into()], "bar".into(), vec![], vec![]).into(),
                        ast::DefaultOutput::new(vec!["$x/baz".into()]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [
                    (
                        "foo/baz".into(),
                        ir_explicit_build(
                            vec!["foo/baz".into()],
                            Rule::new("".into(), None),
                            vec![]
                        )
                        .into()
                    ),
                    (
                        "qux".into(),
                        ir_explicit_build(vec!["qux".into()], Rule::new("".into(), None), vec![])
                            .into()
                    )
                ]
                .into_iter()
                .collect(),
                ["foo/baz".into()].into_iter().collect()
            )
        );
    }

    #[test]
    fn intern_paths() {
        let path_pool = PathPool::new();
        let config = compile(
            &[(
                ROOT_MODULE_PATH.clone(),
                ast::Module::new(vec![
                    ast::VariableDefinition::new("x".into(), "foo".into()).into(),
                    ast_rule("bar", &[("command", "")]).into(),
                    ast_explicit_build(vec!["foo/baz".into()], "bar".into(), vec![], vec![]).into(),
                    ast_explicit_build(
                        vec!["qux".into()],
                        "bar".into(),
                        vec!["$x/baz".into()],
                        vec![],
                    )
                    .into(),
                    ast::DefaultOutput::new(vec!["$x/baz".into()]).into(),
                ]),
            )]
            .into_iter()
            .collect(),
            &DEFAULT_DEPENDENCIES,
            &ROOT_MODULE_PATH,
            &path_pool,
        )
        .unwrap();

        for path in [
            &config.outputs()["foo/baz"].outputs()[0],
            &config.outputs()["qux"].inputs()[0],
            config.default_outputs().get("foo/baz").unwrap(),
        ] {
            assert!(Arc::ptr_eq(path, &path_pool.intern("foo/baz")));
        }
    }

    #[test]
    fn intern_dynamic_paths() {
        let path_pool = PathPool::new();
        let config = compile_dynamic(
            &ast::DynamicModule::new(vec![ast::DynamicBuild::new(
                "foo".into(),
                vec!["bar".into()],
            )]),
            &path_pool,
        )
        .unwrap();
        let (output, build) = config.outputs().iter().next().unwrap();

        assert!(Arc::ptr_eq(output, &path_pool.intern("foo")));
        assert!(Arc::ptr_eq(&build.inputs()[0], &path_pool.intern("bar")));
    }

    #[test]
    fn unescape_dynamic_paths() {
        assert_eq!(
            compile_dynamic(
                &ast::DynamicModule::new(vec![ast::DynamicBuild::new(
                    "C$:\\foo$ bar".into(),
                    vec!["baz$$qux".into()],
                )]),
                &Default::default(),
            )
            .unwrap(),
            DynamicConfig::new(
                [(
                    "C:\\foo bar".into(),
                    DynamicBuild::new(vec!["baz$qux".into()])
                )]
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
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                        ast_explicit_build(vec!["baz".into()], "foo".into(), vec![], vec![]).into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [
                    (
                        "bar".into(),
                        ir_explicit_build(vec!["bar".into()], Rule::new("".into(), None), vec![])
                            .into()
                    ),
                    (
                        "baz".into(),
                        ir_explicit_build(vec!["baz".into()], Rule::new("".into(), None), vec![])
                            .into()
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
                            "foo".into(),
                            vec![],
                            vec![ast::VariableDefinition::new("x".into(), "42".into())]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("42".into(), None), vec![])
                        .into()
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
                        ast_explicit_build(vec!["baz".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "baz".into(),
                    ir_explicit_build(
                        vec!["baz".into()],
                        Rule::new("bar".into(), Some("bar".into())),
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
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("second".into(), None), vec![])
                        .into()
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
                            "foo".into(),
                            vec![],
                            vec![ast::VariableDefinition::new(
                                "command".into(),
                                "build".into()
                            )]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("build".into(), None), vec![])
                        .into()
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
                        ast::VariableDefinition::new("description".into(), "global".into()).into(),
                        ast_rule("foo", &[("command", "")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("".into(), Some("global".into())),
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
                            "foo".into(),
                            vec![],
                            vec![ast::VariableDefinition::new(
                                SOURCE_VARIABLE_NAME.into(),
                                "oh-my-src".into()
                            )]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            Config::new(
                [(
                    "bar".into(),
                    ir_explicit_build(vec!["bar".into()], Rule::new("foo".into(), None), vec![])
                        .into()
                )]
                .into_iter()
                .collect(),
                ["bar".into()].into_iter().collect(),
                [("bar".into(), "oh-my-src".into())].into_iter().collect(),
                Default::default(),
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
                        ast_explicit_build(
                            vec!["foo".into()],
                            "phony".into(),
                            vec!["bar".into()],
                            vec![]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
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
                    ast::Module::new(vec![
                        ast::VariableDefinition::new("builddir".into(), "foo".into()).into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            Config::new(
                Default::default(),
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
                        ast::VariableDefinition::new("x".into(), "foo".into()).into(),
                        ast::VariableDefinition::new("builddir".into(), "$x/bar".into()).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            Config::new(
                Default::default(),
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
                            "phony".into(),
                            vec!["bar".into()],
                            vec![ast::VariableDefinition::new("dyndep".into(), "bar".into())]
                        )
                        .into()
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
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
                        ast_explicit_build(
                            vec!["bar".into()],
                            "foo".into(),
                            vec!["bar.dd".into()],
                            vec![]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    Build::new(
                        vec!["bar".into()],
                        vec![],
                        Some(Rule::new("".into(), None)),
                        vec!["bar.dd".into()],
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
                            "foo".into(),
                            vec!["build.dd".into()],
                            vec![ast::VariableDefinition::new(
                                "dyndep".into(),
                                "build.dd".into()
                            )]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    Build::new(
                        vec!["bar".into()],
                        vec![],
                        Some(Rule::new("".into(), None)),
                        vec!["build.dd".into()],
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
    fn compile_dynamic_module_variable_with_implicit_input() {
        assert_eq!(
            compile_root_module(vec![
                ast::Build::new(
                    vec!["foo".into()],
                    vec![],
                    "phony".into(),
                    vec![],
                    vec!["bar".into()],
                    vec![],
                    vec![ast::VariableDefinition::new("dyndep".into(), "bar".into())]
                )
                .into()
            ])
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
    fn compile_dynamic_module_variable_with_order_only_input() {
        assert_eq!(
            compile_root_module(vec![
                ast::Build::new(
                    vec!["foo".into()],
                    vec![],
                    "phony".into(),
                    vec![],
                    vec![],
                    vec!["bar".into()],
                    vec![ast::VariableDefinition::new("dyndep".into(), "bar".into())]
                )
                .into()
            ])
            .unwrap(),
            create_simple_config(
                [(
                    "foo".into(),
                    Build::new(
                        vec!["foo".into()],
                        vec![],
                        None,
                        vec![],
                        vec!["bar".into()],
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
    fn compile_dynamic_module_variable_with_variable_in_input() {
        assert_eq!(
            compile_root_module(vec![
                ast::VariableDefinition::new("directory".into(), "bar".into()).into(),
                ast_explicit_build(
                    vec!["foo".into()],
                    "phony".into(),
                    vec!["$directory/foo.dd".into()],
                    vec![ast::VariableDefinition::new(
                        "dyndep".into(),
                        "bar/foo.dd".into()
                    )]
                )
                .into()
            ])
            .unwrap(),
            create_simple_config(
                [(
                    "foo".into(),
                    Build::new(
                        vec!["foo".into()],
                        vec![],
                        None,
                        vec!["bar/foo.dd".into()],
                        vec![],
                        Some("bar/foo.dd".into())
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
    fn fail_to_compile_dynamic_module_variable_without_input() {
        assert_eq!(
            compile_root_module(vec![
                ast_explicit_build(
                    vec!["foo".into()],
                    "phony".into(),
                    vec!["baz".into()],
                    vec![ast::VariableDefinition::new("dyndep".into(), "bar".into())]
                )
                .into()
            ]),
            Err(CompileError::DynamicModuleNotInput("bar".into()))
        );
    }

    #[test]
    fn fail_to_compile_rule_level_dynamic_module_variable_without_input() {
        assert_eq!(
            compile_root_module(vec![
                ast_rule("foo", &[("command", ""), ("dyndep", "$out.dd")]).into(),
                ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
            ]),
            Err(CompileError::DynamicModuleNotInput("bar.dd".into()))
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
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                            "foo".into(),
                            vec![],
                            vec![ast::VariableDefinition::new(
                                "depfile".into(),
                                "foo.d".into()
                            )]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                            "foo".into(),
                            vec![],
                            vec![ast::VariableDefinition::new("deps".into(), "gcc".into())]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                            "foo".into(),
                            vec![],
                            vec![ast::VariableDefinition::new("deps".into(), "".into())]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
    fn intern_depfile_paths() {
        let path_pool = PathPool::new();
        let config = compile(
            &[(
                ROOT_MODULE_PATH.clone(),
                ast::Module::new(vec![
                    ast_rule("foo", &[("command", ""), ("depfile", "foo.d")]).into(),
                    ast_rule(
                        "bar",
                        &[("command", ""), ("depfile", "bar.d"), ("deps", "gcc")],
                    )
                    .into(),
                    ast_explicit_build(vec!["baz".into()], "foo".into(), vec![], vec![]).into(),
                    ast_explicit_build(vec!["qux".into()], "bar".into(), vec![], vec![]).into(),
                ]),
            )]
            .into_iter()
            .collect(),
            &DEFAULT_DEPENDENCIES,
            &ROOT_MODULE_PATH,
            &path_pool,
        )
        .unwrap();

        for (output, depfile) in [("baz", "foo.d"), ("qux", "bar.d")] {
            assert!(matches!(
                config.outputs()[output].rule().unwrap().header_dependency(),
                Some(HeaderDependency::Make { path } | HeaderDependency::Gcc { path })
                    if Arc::ptr_eq(path, &path_pool.intern(depfile))
            ));
        }
    }

    #[test]
    fn fail_to_compile_gcc_header_dependency_without_depfile() {
        assert_eq!(
            compile(
                &[(
                    ROOT_MODULE_PATH.clone(),
                    ast::Module::new(vec![
                        ast_rule("foo", &[("command", "bar"), ("deps", "gcc")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
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
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
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
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                            "msvc_deps_prefix".into(),
                            "Hinweis: Einlesen der Datei ".into()
                        )
                        .into(),
                        ast_rule("foo", &[("command", "bar"), ("deps", "msvc")]).into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                        ast::VariableDefinition::new(
                            "msvc_deps_prefix".into(),
                            "global prefix: ".into()
                        )
                        .into(),
                        ast_rule(
                            "foo",
                            &[
                                ("command", "bar"),
                                ("deps", "msvc"),
                                ("msvc_deps_prefix", "rule prefix: ")
                            ]
                        )
                        .into(),
                        ast_explicit_build(vec!["bar".into()], "foo".into(), vec![], vec![]).into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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
                            "foo".into(),
                            vec![],
                            vec![ast::VariableDefinition::new(
                                "msvc_deps_prefix".into(),
                                "build prefix: ".into()
                            )]
                        )
                        .into(),
                    ])
                )]
                .into_iter()
                .collect(),
                &DEFAULT_DEPENDENCIES,
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
            .unwrap(),
            create_simple_config(
                [(
                    "bar".into(),
                    ir_explicit_build(
                        vec!["bar".into()],
                        Rule::new("bar".into(), None).with_header_dependency(Some(
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

    #[test]
    fn compile_rule_level_pool() {
        assert_eq!(
            compile_root_module(vec![
                ast::Pool::new("foo".into(), "2".into()).into(),
                ast_rule("qux", &[("command", "baz"), ("pool", "foo")]).into(),
                ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into(),
            ]),
            Ok(create_pool_config(limited_pool("foo"), &[("foo", 2)]))
        );
    }

    #[test]
    fn compile_build_level_pool() {
        assert_eq!(
            compile_root_module(vec![
                ast::Pool::new("foo".into(), "2".into()).into(),
                ast_rule("qux", &[("command", "baz")]).into(),
                ast_explicit_build(
                    vec!["bar".into()],
                    "qux".into(),
                    vec![],
                    vec![ast::VariableDefinition::new("pool".into(), "foo".into())]
                )
                .into(),
            ]),
            Ok(create_pool_config(limited_pool("foo"), &[("foo", 2)]))
        );
    }

    #[test]
    fn compile_build_level_pool_shadowing_rule_level_pool() {
        assert_eq!(
            compile_root_module(vec![
                ast::Pool::new("foo".into(), "2".into()).into(),
                ast_rule("qux", &[("command", "baz"), ("pool", "console")]).into(),
                ast_explicit_build(
                    vec!["bar".into()],
                    "qux".into(),
                    vec![],
                    vec![ast::VariableDefinition::new("pool".into(), "foo".into())]
                )
                .into(),
            ]),
            Ok(create_pool_config(limited_pool("foo"), &[("foo", 2)]))
        );
    }

    #[test]
    fn compile_build_level_empty_pool_shadowing_rule_level_pool() {
        assert_eq!(
            compile_root_module(vec![
                ast_rule("qux", &[("command", "baz"), ("pool", "console")]).into(),
                ast_explicit_build(
                    vec!["bar".into()],
                    "qux".into(),
                    vec![],
                    vec![ast::VariableDefinition::new("pool".into(), "".into())]
                )
                .into(),
            ]),
            Ok(create_pool_config(None, &[]))
        );
    }

    #[test]
    fn compile_console_pool() {
        assert_eq!(
            compile_root_module(vec![
                ast_rule("qux", &[("command", "baz"), ("pool", "console")]).into(),
                ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into(),
            ]),
            Ok(create_pool_config(Some(Pool::Console), &[]))
        );
    }

    #[test]
    fn compile_console_pool_in_phony_build() {
        assert_eq!(
            compile_root_module(vec![
                ast_explicit_build(
                    vec!["foo".into()],
                    "phony".into(),
                    vec![],
                    vec![ast::VariableDefinition::new(
                        "pool".into(),
                        "console".into()
                    )]
                )
                .into(),
            ]),
            Ok(create_simple_config(
                [(
                    "foo".into(),
                    Build::new(vec!["foo".into()], vec![], None, vec![], vec![], None).into()
                )]
                .into_iter()
                .collect(),
                ["foo".into()].into_iter().collect()
            ))
        );
    }

    #[test]
    fn compile_pool_with_zero_depth() {
        assert_eq!(
            compile_root_module(vec![
                ast::Pool::new("foo".into(), "0".into()).into(),
                ast_rule("qux", &[("command", "baz"), ("pool", "foo")]).into(),
                ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into(),
            ]),
            Ok(create_pool_config(None, &[]))
        );
    }

    #[test]
    fn compile_pool_depth_with_variable() {
        assert_eq!(
            compile_root_module(vec![
                ast::VariableDefinition::new("x".into(), "2".into()).into(),
                ast::Pool::new("foo".into(), "$x".into()).into(),
                ast::VariableDefinition::new("x".into(), "3".into()).into(),
                ast_rule("qux", &[("command", "baz"), ("pool", "foo")]).into(),
                ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into(),
            ]),
            Ok(create_pool_config(limited_pool("foo"), &[("foo", 2)]))
        );
    }

    #[test]
    fn compile_pool_depth_with_empty_variable() {
        assert_eq!(
            compile_root_module(vec![
                ast::VariableDefinition::new("x".into(), "".into()).into(),
                ast::Pool::new("foo".into(), "$x 2".into()).into(),
                ast_rule("qux", &[("command", "baz"), ("pool", "foo")]).into(),
                ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into(),
            ]),
            Ok(create_pool_config(limited_pool("foo"), &[("foo", 2)]))
        );
    }

    #[test]
    fn compile_pool_name_with_variable() {
        assert_eq!(
            compile_root_module(vec![
                ast::Pool::new("foo".into(), "2".into()).into(),
                ast_rule("qux", &[("command", "baz"), ("pool", "$x")]).into(),
                ast_explicit_build(
                    vec!["bar".into()],
                    "qux".into(),
                    vec![],
                    vec![ast::VariableDefinition::new("x".into(), "foo".into())]
                )
                .into(),
            ]),
            Ok(create_pool_config(limited_pool("foo"), &[("foo", 2)]))
        );
    }

    #[test]
    fn do_not_interpolate_out_variable_in_pool() {
        assert_eq!(
            compile_root_module(vec![
                ast::Pool::new("bar".into(), "2".into()).into(),
                ast_rule("qux", &[("command", "baz"), ("pool", "$out")]).into(),
                ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into(),
            ]),
            Ok(create_pool_config(None, &[("bar", 2)]))
        );
    }

    #[test]
    fn fail_to_compile_unknown_pool() {
        assert_eq!(
            compile_root_module(vec![
                ast_rule("qux", &[("command", "baz"), ("pool", "foo")]).into(),
                ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into(),
            ]),
            Err(CompileError::PoolNotFound("foo".into()))
        );
    }

    #[test]
    fn fail_to_compile_unknown_pool_in_phony_build() {
        assert_eq!(
            compile_root_module(vec![
                ast_explicit_build(
                    vec!["bar".into()],
                    "phony".into(),
                    vec![],
                    vec![ast::VariableDefinition::new("pool".into(), "foo".into())]
                )
                .into(),
            ]),
            Err(CompileError::PoolNotFound("foo".into()))
        );
    }

    #[test]
    fn fail_to_compile_pool_declared_after_build() {
        assert_eq!(
            compile_root_module(vec![
                ast_rule("qux", &[("command", "baz"), ("pool", "foo")]).into(),
                ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into(),
                ast::Pool::new("foo".into(), "2".into()).into(),
            ]),
            Err(CompileError::PoolNotFound("foo".into()))
        );
    }

    #[test]
    fn fail_to_compile_duplicate_pool() {
        assert_eq!(
            compile_root_module(vec![
                ast::Pool::new("foo".into(), "1".into()).into(),
                ast::Pool::new("foo".into(), "2".into()).into(),
            ]),
            Err(CompileError::DuplicatePool("foo".into()))
        );
    }

    #[test]
    fn fail_to_compile_duplicate_console_pool() {
        assert_eq!(
            compile_root_module(vec![ast::Pool::new("console".into(), "foo".into()).into()]),
            Err(CompileError::DuplicatePool("console".into()))
        );
    }

    #[test]
    fn fail_to_compile_invalid_pool_depth() {
        for depth in ["", "foo", "-1", "1x", "99999999999999999999"] {
            assert_eq!(
                compile_root_module(vec![ast::Pool::new("foo".into(), depth.into()).into()]),
                Err(CompileError::InvalidPoolDepth("foo".into(), depth.into()))
            );
        }
    }

    mod submodule {
        use super::*;
        use pretty_assertions::assert_eq;

        const SUBMODULE_PATH: &str = "foo.ninja";

        fn compile_with_submodule(
            statements: Vec<ast::Statement>,
            submodule_statements: Vec<ast::Statement>,
        ) -> Result<Config, CompileError> {
            compile(
                &[
                    (ROOT_MODULE_PATH.clone(), ast::Module::new(statements)),
                    (
                        SUBMODULE_PATH.into(),
                        ast::Module::new(submodule_statements),
                    ),
                ]
                .into_iter()
                .collect(),
                &[(
                    ROOT_MODULE_PATH.clone(),
                    [(SUBMODULE_PATH.into(), PathBuf::from(SUBMODULE_PATH))]
                        .into_iter()
                        .collect(),
                )]
                .into_iter()
                .collect(),
                &ROOT_MODULE_PATH,
                &Default::default(),
            )
        }

        #[test]
        fn reference_variable_in_parent_module() {
            assert_eq!(
                compile(
                    &[
                        (
                            ROOT_MODULE_PATH.clone(),
                            ast::Module::new(vec![
                                ast::VariableDefinition::new("x".into(), "42".into()).into(),
                                ast::Submodule::new(SUBMODULE_PATH.into()).into(),
                            ])
                        ),
                        (
                            SUBMODULE_PATH.into(),
                            ast::Module::new(vec![
                                ast_rule("foo", &[("command", "$x")]).into(),
                                ast_explicit_build(
                                    vec!["bar".into()],
                                    "foo".into(),
                                    vec![],
                                    vec![]
                                )
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
                    &ROOT_MODULE_PATH,
                    &Default::default(),
                )
                .unwrap(),
                create_simple_config(
                    [(
                        "bar".into(),
                        ir_explicit_build(vec!["bar".into()], Rule::new("42".into(), None), vec![])
                            .into()
                    )]
                    .into_iter()
                    .collect(),
                    ["bar".into()].into_iter().collect()
                )
            );
        }

        #[test]
        fn reference_rule_in_parent_module() {
            assert_eq!(
                compile(
                    &[
                        (
                            ROOT_MODULE_PATH.clone(),
                            ast::Module::new(vec![
                                ast::VariableDefinition::new("x".into(), "42".into()).into(),
                                ast_rule("foo", &[("command", "$x")]).into(),
                                ast::Submodule::new(SUBMODULE_PATH.into()).into(),
                            ])
                        ),
                        (
                            SUBMODULE_PATH.into(),
                            ast::Module::new(vec![
                                ast_explicit_build(
                                    vec!["bar".into()],
                                    "foo".into(),
                                    vec![],
                                    vec![]
                                )
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
                    &ROOT_MODULE_PATH,
                    &Default::default(),
                )
                .unwrap(),
                create_simple_config(
                    [(
                        "bar".into(),
                        ir_explicit_build(vec!["bar".into()], Rule::new("42".into(), None), vec![])
                            .into()
                    )]
                    .into_iter()
                    .collect(),
                    ["bar".into()].into_iter().collect()
                )
            );
        }

        #[test]
        fn do_not_overwrite_variable_in_parent_module() {
            assert_eq!(
                compile(
                    &[
                        (
                            ROOT_MODULE_PATH.clone(),
                            ast::Module::new(vec![
                                ast::VariableDefinition::new("x".into(), "42".into()).into(),
                                ast_rule("foo", &[("command", "$x")]).into(),
                                ast::Submodule::new(SUBMODULE_PATH.into()).into(),
                                ast_explicit_build(
                                    vec!["bar".into()],
                                    "foo".into(),
                                    vec![],
                                    vec![]
                                )
                                .into(),
                            ])
                        ),
                        (
                            SUBMODULE_PATH.into(),
                            ast::Module::new(vec![
                                ast::VariableDefinition::new("x".into(), "13".into()).into(),
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
                    &ROOT_MODULE_PATH,
                    &Default::default(),
                )
                .unwrap(),
                create_simple_config(
                    [(
                        "bar".into(),
                        ir_explicit_build(vec!["bar".into()], Rule::new("42".into(), None), vec![])
                            .into()
                    )]
                    .into_iter()
                    .collect(),
                    ["bar".into()].into_iter().collect()
                )
            );
        }

        #[test]
        fn reference_pool_in_parent_module() {
            assert_eq!(
                compile_with_submodule(
                    vec![
                        ast::Pool::new("foo".into(), "2".into()).into(),
                        ast_rule("qux", &[("command", "baz"), ("pool", "foo")]).into(),
                        ast::Submodule::new(SUBMODULE_PATH.into()).into(),
                    ],
                    vec![
                        ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into()
                    ],
                ),
                Ok(create_pool_config(limited_pool("foo"), &[("foo", 2)]))
            );
        }

        #[test]
        fn reference_pool_in_child_module() {
            assert_eq!(
                compile_with_submodule(
                    vec![
                        ast::Submodule::new(SUBMODULE_PATH.into()).into(),
                        ast_rule("qux", &[("command", "baz"), ("pool", "foo")]).into(),
                        ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into(),
                    ],
                    vec![ast::Pool::new("foo".into(), "2".into()).into()],
                ),
                Ok(create_pool_config(limited_pool("foo"), &[("foo", 2)]))
            );
        }

        #[test]
        fn fail_to_reference_pool_declared_after_submodule() {
            assert_eq!(
                compile_with_submodule(
                    vec![
                        ast_rule("qux", &[("command", "baz"), ("pool", "foo")]).into(),
                        ast::Submodule::new(SUBMODULE_PATH.into()).into(),
                        ast::Pool::new("foo".into(), "2".into()).into(),
                    ],
                    vec![
                        ast_explicit_build(vec!["bar".into()], "qux".into(), vec![], vec![]).into()
                    ],
                ),
                Err(CompileError::PoolNotFound("foo".into()))
            );
        }

        #[test]
        fn fail_to_declare_pool_in_submodule_referenced_twice() {
            assert_eq!(
                compile_with_submodule(
                    vec![
                        ast::Submodule::new(SUBMODULE_PATH.into()).into(),
                        ast::Submodule::new(SUBMODULE_PATH.into()).into(),
                    ],
                    vec![ast::Pool::new("foo".into(), "2".into()).into()],
                ),
                Err(CompileError::DuplicatePool("foo".into()))
            );
        }
    }
}

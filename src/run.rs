mod context;
mod file_cache;
mod hash;
mod header_dependency;
mod log;
mod options;

use self::{
    context::RunContext,
    hash::{calculate_content_hash, calculate_timestamp_hash, hash_content},
    header_dependency::{exclude_show_includes, read_header_dependencies},
};
use crate::{
    build_graph::{BuildGraph, BuildGraphError},
    compile::compile_dynamic,
    context::Context,
    debug,
    error::BuildError,
    hash_type::HashType,
    ir::{Build, Config, Rule},
    parse::parse_dynamic,
    profile,
};
use alloc::sync::Arc;
use async_recursion::async_recursion;
use futures::future::{FutureExt, try_join_all};
use itertools::{Either, Itertools};
pub use options::RunOptions;
use std::{
    path::{Path, PathBuf},
    process::Output,
    time::SystemTime,
};
use tokio::{join, spawn, time::Instant, try_join};

type FileState<T> = Result<Option<T>, BuildError>;

/// Runs builds.
pub async fn run(
    context: &Arc<Context>,
    config: Arc<Config>,
    outputs: &[String],
    options: RunOptions,
) -> Result<(), BuildError> {
    let mut graph = BuildGraph::new(config.outputs());

    for build in config
        .outputs()
        .values()
        .filter(|build| build.rule().is_some())
        .unique_by(|build| build.id())
    {
        graph.add_header_dependencies(
            &build.outputs()[0],
            &context.database().get_header_dependencies(build.id())?,
        );
    }

    let context = Arc::new(RunContext::new(context.clone(), config, graph, options));

    context
        .build_graph()
        .lock()
        .await
        .validate()
        .map_err(|error| map_build_graph_error(&context, &error))?;

    let result = try_join_all(
        if outputs.is_empty() {
            context
                .config()
                .default_outputs()
                .iter()
                .map(|output| {
                    context
                        .config()
                        .outputs()
                        .get(output.as_ref())
                        .ok_or_else(|| BuildError::DefaultOutputNotFound(output.clone()))
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            outputs
                .iter()
                .map(|output| {
                    context
                        .config()
                        .outputs()
                        .get(output.as_str())
                        .ok_or_else(|| BuildError::OutputNotFound(output.clone()))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        .into_iter()
        .map(|build| run_build(context.clone(), build)),
    )
    .await;

    context.application().database().flush().await?;

    result.map(|_| ())
}

#[async_recursion]
async fn run_build(context: Arc<RunContext>, build: &Arc<Build>) -> Result<(), BuildError> {
    // Do not inline this to avoid holding a lock of build futures across an await point.
    let future = context
        .build_futures()
        .entry_async(build.id())
        .await
        .or_insert_with(|| spawn_build(context.clone(), build.clone()).boxed().shared())
        .get()
        .clone();

    future.await
}

async fn spawn_build(context: Arc<RunContext>, build: Arc<Build>) -> Result<(), BuildError> {
    spawn(async move {
        let outputs = build
            .outputs()
            .iter()
            .chain(build.implicit_outputs())
            .map(AsRef::as_ref)
            .collect::<Vec<_>>();
        let outputs_exist = build_inputs(
            &context,
            &build
                .inputs()
                .iter()
                .chain(build.order_only_inputs())
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            &outputs,
        )
        .await?
        .iter()
        .all(|state| matches!(state, Ok(Some(_))));

        // TODO Consider caching dynamic modules.
        let dynamic_config = if let Some(dynamic_module) = build.dynamic_module() {
            let mut source = String::new();
            context
                .application()
                .file_system()
                .read_file_to_string(dynamic_module.as_ref().as_ref(), &mut source)
                .await?;
            let config = compile_dynamic(&parse_dynamic(&source)?)?;

            context
                .build_graph()
                .lock()
                .await
                .validate_dynamic(&config)
                .map_err(|error| map_build_graph_error(&context, &error))?;

            Some(config)
        } else {
            None
        };

        let dynamic_inputs = if let Some(config) = &dynamic_config {
            build
                .outputs()
                .iter()
                .chain(build.implicit_outputs())
                .find_map(|output| config.outputs().get(output.as_ref()))
                .map(|build| build.inputs())
                .ok_or_else(|| BuildError::DynamicDependencyNotFound(build.clone()))?
        } else {
            &[]
        };

        build_inputs(
            &context,
            &dynamic_inputs.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            &[],
        )
        .await?;

        // Commands of inputs can update outputs of phony builds.
        if build.rule().is_none() {
            for output in &outputs {
                context.file_cache().invalidate(output).await;
            }
        }

        let header_dependencies = if build.rule().is_some() {
            let dependencies = context
                .application()
                .database()
                .get_header_dependencies(build.id())?;

            try_join_all(
                dependencies
                    .iter()
                    .filter_map(|dependency| context.config().outputs().get(dependency.as_str()))
                    .map(|build| run_build(context.clone(), build)),
            )
            .await?;

            filter_existing_header_dependencies(&context, &dependencies).await?
        } else {
            vec![]
        };
        let (phony_inputs, file_inputs) =
            classify_inputs(&context, &build, dynamic_inputs, &header_dependencies);
        let stored_timestamp_hash = if outputs_exist {
            context
                .application()
                .database()
                .get_hash(HashType::Timestamp, build.id())?
        } else {
            None
        };
        // Content hashes are always needed without a stored timestamp hash.
        let (modified_times, content_hashes) = if stored_timestamp_hash.is_some() {
            (get_modified_times(&context, &file_inputs).await, None)
        } else {
            let (modified_times, content_hashes) = join!(
                get_modified_times(&context, &file_inputs),
                get_content_hashes(&context, &file_inputs)
            );

            (modified_times, Some(content_hashes))
        };
        let modified_times = collect_modified_times(&context, &file_inputs, modified_times)?;

        if Some(calculate_timestamp_hash(
            &context,
            &build,
            &modified_times,
            &phony_inputs,
        )?) == stored_timestamp_hash
        {
            return Ok(());
        }

        let content_hashes = match content_hashes {
            Some(content_hashes) => content_hashes,
            None => get_content_hashes(&context, &file_inputs).await,
        };
        let (mut timestamp_hash, mut content_hash) = calculate_hashes(
            &context,
            &build,
            dynamic_inputs,
            &file_inputs,
            modified_times,
            content_hashes,
            &phony_inputs,
        )?;

        if outputs_exist
            && Some(content_hash)
                == context
                    .application()
                    .database()
                    .get_hash(HashType::Content, build.id())?
        {
            return Ok(());
        } else if let Some(rule) = build.rule() {
            prepare_directories(&context, &outputs).await?;

            let output = run_rule_and_invalidate_outputs(&context, rule, &outputs).await?;
            let new_header_dependencies = read_header_dependencies(&context, rule, &output).await?;

            context
                .application()
                .database()
                .set_header_dependencies(build.id(), &new_header_dependencies)?;

            for output in build.outputs() {
                context.application().database().set_output(output)?;

                if let Some(source) = context.config().source_map().get(output) {
                    context
                        .application()
                        .database()
                        .set_source(output, source)?;
                }
            }

            if header_dependencies != new_header_dependencies {
                let header_dependencies =
                    filter_existing_header_dependencies(&context, &new_header_dependencies).await?;
                let (phony_inputs, file_inputs) =
                    classify_inputs(&context, &build, dynamic_inputs, &header_dependencies);
                let (modified_times, content_hashes) = join!(
                    get_modified_times(&context, &file_inputs),
                    get_content_hashes(&context, &file_inputs)
                );

                (timestamp_hash, content_hash) = calculate_hashes(
                    &context,
                    &build,
                    dynamic_inputs,
                    &file_inputs,
                    collect_modified_times(&context, &file_inputs, modified_times)?,
                    content_hashes,
                    &phony_inputs,
                )?;
            }
        }

        context.application().database().set_hash(
            HashType::Timestamp,
            build.id(),
            timestamp_hash,
        )?;
        context
            .application()
            .database()
            .set_hash(HashType::Content, build.id(), content_hash)?;

        Ok(())
    })
    .await?
}

// Builds inputs, checks existence of source inputs, and returns states of the
// given paths.
async fn build_inputs(
    context: &Arc<RunContext>,
    inputs: &[&str],
    paths: &[&str],
) -> Result<Vec<FileState<SystemTime>>, BuildError> {
    let (builds, sources): (Vec<_>, Vec<_>) = inputs.iter().partition_map(|&input| {
        context
            .config()
            .outputs()
            .get(input)
            .map_or(Either::Right(input), Either::Left)
    });

    let ((), states) = try_join!(
        async {
            try_join_all(
                builds
                    .into_iter()
                    .map(|build| run_build(context.clone(), build)),
            )
            .await?;

            Ok::<_, BuildError>(())
        },
        async {
            let mut states = get_modified_times(
                context,
                &sources.iter().chain(paths).copied().collect::<Vec<_>>(),
            )
            .await;
            let path_states = states.split_off(sources.len());

            for (source, state) in sources.iter().zip(states) {
                if state?.is_none() {
                    return Err(file_not_found(context, source));
                }
            }

            Ok(path_states)
        }
    )?;

    Ok(states)
}

async fn filter_existing_header_dependencies(
    context: &Arc<RunContext>,
    dependencies: &[String],
) -> Result<Vec<String>, BuildError> {
    dependencies
        .iter()
        .zip(
            get_modified_times(
                context,
                &dependencies.iter().map(String::as_str).collect::<Vec<_>>(),
            )
            .await,
        )
        .filter_map(|(dependency, state)| {
            state
                .map(|time| time.map(|_| dependency.clone()))
                .transpose()
        })
        .collect()
}

async fn prepare_directories(context: &RunContext, outputs: &[&str]) -> Result<(), BuildError> {
    try_join_all(
        outputs
            .iter()
            .filter_map(|output| Path::new(output).parent())
            .filter(|directory| !directory.as_os_str().is_empty())
            .unique()
            .map(|directory| async move {
                context
                    .application()
                    .file_system()
                    .create_directory(directory)
                    .await
                    .map_err(BuildError::from)
            }),
    )
    .await?;

    Ok(())
}

async fn get_modified_times(
    context: &Arc<RunContext>,
    paths: &[&str],
) -> Vec<FileState<SystemTime>> {
    context
        .file_cache()
        .modified_times()
        .get(paths, |paths| {
            let context = context.clone();

            async move {
                Ok(context
                    .application()
                    .file_system()
                    .modified_times(to_paths(&paths))
                    .await?
                    .into_iter()
                    .map(|result| result.map_err(BuildError::from))
                    .collect())
            }
        })
        .await
}

async fn get_content_hashes(context: &Arc<RunContext>, paths: &[&str]) -> Vec<FileState<u64>> {
    context
        .file_cache()
        .content_hashes()
        .get(paths, |paths| {
            let context = context.clone();

            async move {
                Ok(context
                    .application()
                    .file_system()
                    .hash_files(to_paths(&paths), hash_content)
                    .await?
                    .into_iter()
                    .map(|result| result.map_err(BuildError::from))
                    .collect())
            }
        })
        .await
}

fn to_paths(paths: &[Arc<str>]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|path| PathBuf::from(path.as_ref()))
        .collect()
}

fn collect_modified_times(
    context: &RunContext,
    paths: &[&str],
    states: Vec<FileState<SystemTime>>,
) -> Result<Vec<SystemTime>, BuildError> {
    paths
        .iter()
        .zip(states)
        .map(|(path, state)| state?.ok_or_else(|| file_not_found(context, path)))
        .collect()
}

fn calculate_hashes(
    context: &RunContext,
    build: &Build,
    dynamic_inputs: &[Arc<str>],
    file_inputs: &[&str],
    modified_times: Vec<SystemTime>,
    content_hashes: Vec<FileState<u64>>,
    phony_inputs: &[&str],
) -> Result<(u64, u64), BuildError> {
    let (modified_times, content_hashes): (Vec<_>, Vec<_>) = retain_existing_files(
        file_inputs.iter().copied().zip(
            modified_times
                .into_iter()
                .zip(content_hashes.into_iter().collect::<Result<Vec<_>, _>>()?),
        ),
        &build
            .inputs()
            .iter()
            .chain(dynamic_inputs)
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
    )
    .map_err(|input| file_not_found(context, input))?
    .into_iter()
    .unzip();

    Ok((
        calculate_timestamp_hash(context, build, &modified_times, phony_inputs)?,
        calculate_content_hash(context, build, &content_hashes, phony_inputs)?,
    ))
}

// Drops header dependencies deleted by other builds after their existence checks
// but fails on missing explicit or dynamic inputs.
fn retain_existing_files<'a>(
    files: impl IntoIterator<Item = (&'a str, (SystemTime, Option<u64>))>,
    explicit_inputs: &[&str],
) -> Result<Vec<(SystemTime, u64)>, &'a str> {
    files
        .into_iter()
        .filter_map(|(file, (modified_time, content_hash))| {
            content_hash.map_or_else(
                || explicit_inputs.contains(&file).then_some(Err(file)),
                |content_hash| Some(Ok((modified_time, content_hash))),
            )
        })
        .collect()
}

fn file_not_found(context: &RunContext, path: &str) -> BuildError {
    context
        .application()
        .database()
        .get_source(path)
        .map_or_else(Into::into, |source| {
            BuildError::FileNotFound(source.unwrap_or_else(|| path.into()))
        })
}

fn classify_inputs<'a>(
    context: &'a RunContext,
    build: &'a Build,
    dynamic_inputs: &'a [Arc<str>],
    header_dependencies: &'a [String],
) -> (Vec<&'a str>, Vec<&'a str>) {
    let (phony_inputs, file_inputs) = build
        .inputs()
        .iter()
        .chain(dynamic_inputs)
        .map(AsRef::as_ref)
        .unique()
        .partition::<Vec<_>, _>(|&input| {
            context
                .config()
                .outputs()
                .get(input)
                .map_or_default(|build| build.rule().is_none())
        });

    (
        phony_inputs,
        file_inputs
            .into_iter()
            .chain(header_dependencies.iter().map(String::as_str))
            .unique()
            .collect(),
    )
}

// Invalidates outputs even if a command fails because it can still update them.
async fn run_rule_and_invalidate_outputs(
    context: &RunContext,
    rule: &Rule,
    outputs: &[&str],
) -> Result<Output, BuildError> {
    let output = run_rule(context, rule).await;

    for path in outputs {
        context.file_cache().invalidate(path).await;
    }

    output
}

async fn run_rule(context: &RunContext, rule: &Rule) -> Result<Output, BuildError> {
    let ((output, duration), mut console) = try_join!(
        async {
            let start_time = Instant::now();
            let output = context
                .application()
                .command_runner()
                .run(rule.command())
                .await?;

            Ok::<_, BuildError>((output, Instant::now() - start_time))
        },
        async {
            let mut console = context.application().console().lock().await;

            if let Some(description) = rule.description() {
                console.write_stderr(description.as_bytes()).await?;
                console.write_stderr(b"\n").await?;
            }

            debug!(context, console, "command: {}", rule.command());

            Ok(console)
        }
    )?;

    profile!(context, console, "duration: {} ms", duration.as_millis());

    console
        .write_stdout(&exclude_show_includes(rule, &output.stdout))
        .await?;
    console.write_stderr(&output.stderr).await?;

    if !output.status.success() {
        debug!(
            context,
            console,
            "exit status: {}",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "-".into())
        );

        return Err(BuildError::Build);
    }

    Ok(output)
}

fn map_build_graph_error(context: &RunContext, error: &BuildGraphError) -> BuildError {
    match error {
        BuildGraphError::CircularDependency(outputs) => {
            match outputs
                .iter()
                .map(|output| {
                    Ok(context
                        .application()
                        .database()
                        .get_source(output)?
                        .map(|string| string.into())
                        .unwrap_or_else(|| output.clone()))
                })
                .collect::<Result<Vec<_>, BuildError>>()
            {
                Ok(outputs) => {
                    BuildGraphError::CircularDependency(outputs.into_iter().dedup().collect())
                        .into()
                }
                Err(error) => error,
            }
        }
        BuildGraphError::OutputNotFound(_) => error.clone().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        infrastructure::{FakeCommandRunner, FakeConsole, FakeDatabase, FakeFileSystem},
        ir::HeaderDependency,
    };
    use pretty_assertions::assert_eq;
    use regex::Regex;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    use std::{collections::HashMap, process::ExitStatus};
    use tokio::task::yield_now;

    const DEFAULT_OPTIONS: RunOptions = RunOptions {
        debug: false,
        profile: false,
    };

    fn create_context(
        command_runner: &FakeCommandRunner,
        console: &FakeConsole,
        file_system: &FakeFileSystem,
    ) -> Arc<Context> {
        Context::new(
            command_runner.clone(),
            console.clone(),
            FakeDatabase::default(),
            file_system.clone(),
        )
        .into()
    }

    fn create_outputs(builds: Vec<Build>) -> HashMap<Arc<str>, Arc<Build>> {
        builds
            .into_iter()
            .map(Arc::new)
            .flat_map(|build| {
                build
                    .outputs()
                    .iter()
                    .chain(build.implicit_outputs())
                    .map(|output| (output.clone(), build.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn create_simple_config(builds: Vec<Build>, default_outputs: &[&str]) -> Arc<Config> {
        Config::new(
            create_outputs(builds),
            default_outputs
                .iter()
                .map(|&output| output.into())
                .collect(),
            Default::default(),
            None,
        )
        .into()
    }

    fn explicit_build(outputs: Vec<Arc<str>>, rule: Rule, inputs: Vec<Arc<str>>) -> Build {
        Build::new(outputs, vec![], rule.into(), inputs, vec![], None)
    }

    fn failed_output() -> Output {
        Output {
            status: ExitStatus::from_raw(cfg_select! {
                unix => 1 << 8,
                windows => 1,
            }),
            stdout: vec![],
            stderr: vec![],
        }
    }

    #[tokio::test]
    async fn build_nothing() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_simple_config(vec![], &[]),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn build_default_output() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_simple_config(
                vec![
                    explicit_build(vec!["foo".into()], Rule::new("touch foo", None), vec![]),
                    explicit_build(vec!["bar".into()], Rule::new("touch bar", None), vec![]),
                ],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands(), ["touch foo"]);
    }

    #[tokio::test]
    async fn build_specified_output() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_simple_config(
                vec![
                    explicit_build(vec!["foo".into()], Rule::new("touch foo", None), vec![]),
                    explicit_build(vec!["bar".into()], Rule::new("touch bar", None), vec![]),
                ],
                &["foo"],
            ),
            &["bar".into()],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands(), ["touch bar"]);
    }

    #[tokio::test]
    async fn build_duplicate_outputs() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_simple_config(
                vec![explicit_build(
                    vec!["foo".into()],
                    Rule::new("touch foo", None),
                    vec![],
                )],
                &[],
            ),
            &["foo".into(), "foo".into()],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands(), ["touch foo"]);
    }

    #[tokio::test]
    async fn fail_with_unknown_default_output() {
        assert_eq!(
            run(
                &create_context(
                    &Default::default(),
                    &Default::default(),
                    &Default::default()
                ),
                create_simple_config(vec![], &["foo"]),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::DefaultOutputNotFound("foo".into()))
        );
    }

    #[tokio::test]
    async fn fail_with_unknown_output() {
        assert_eq!(
            run(
                &create_context(
                    &Default::default(),
                    &Default::default(),
                    &Default::default()
                ),
                create_simple_config(vec![], &[]),
                &["foo".into()],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::OutputNotFound("foo".into()))
        );
    }

    #[tokio::test]
    async fn fail_with_unknown_output_after_known_output() {
        let command_runner = FakeCommandRunner::default();

        assert_eq!(
            run(
                &create_context(&command_runner, &Default::default(), &Default::default()),
                create_simple_config(
                    vec![explicit_build(
                        vec!["foo".into()],
                        Rule::new("touch foo", None),
                        vec![],
                    )],
                    &[],
                ),
                &["foo".into(), "bar".into()],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::OutputNotFound("bar".into()))
        );

        // Let the runtime run builds spawned before the error if any.
        yield_now().await;

        assert_eq!(command_runner.commands(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn build_shared_input_once() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();

        file_system.write_file("bar", "");
        file_system.write_file("baz", "");

        run(
            &create_context(&command_runner, &Default::default(), &file_system),
            create_simple_config(
                vec![
                    explicit_build(
                        vec!["foo".into()],
                        Rule::new("touch foo", None),
                        vec!["bar".into(), "baz".into()],
                    ),
                    explicit_build(
                        vec!["bar".into()],
                        Rule::new("touch bar", None),
                        vec!["baz".into()],
                    ),
                    explicit_build(vec!["baz".into()], Rule::new("touch baz", None), vec![]),
                ],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(
            command_runner.commands(),
            ["touch baz", "touch bar", "touch foo"]
        );
    }

    #[tokio::test]
    async fn build_order_only_input() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_simple_config(
                vec![
                    Build::new(
                        vec!["foo".into()],
                        vec![],
                        Rule::new("touch foo", None).into(),
                        vec![],
                        vec!["bar".into()],
                        None,
                    ),
                    explicit_build(vec!["bar".into()], Rule::new("touch bar", None), vec![]),
                ],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands(), ["touch bar", "touch foo"]);
    }

    #[tokio::test]
    async fn build_multiple_outputs_of_build() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");

        run(
            &create_context(&command_runner, &Default::default(), &file_system),
            create_simple_config(
                vec![
                    explicit_build(
                        vec!["foo".into(), "bar".into()],
                        Rule::new("touch foo bar", None),
                        vec![],
                    ),
                    explicit_build(
                        vec!["baz".into()],
                        Rule::new("cat foo bar > baz", None),
                        vec!["foo".into(), "bar".into()],
                    ),
                ],
                &["baz"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(
            command_runner.commands(),
            ["touch foo bar", "cat foo bar > baz"]
        );
    }

    #[tokio::test]
    async fn fail_with_missing_input() {
        let command_runner = FakeCommandRunner::default();

        assert_eq!(
            run(
                &create_context(&command_runner, &Default::default(), &Default::default()),
                create_simple_config(
                    vec![explicit_build(
                        vec!["foo".into()],
                        Rule::new("cp bar foo", None),
                        vec!["bar".into()],
                    )],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::FileNotFound("bar".into()))
        );
        assert_eq!(command_runner.commands(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn fail_with_missing_order_only_input() {
        assert_eq!(
            run(
                &create_context(
                    &Default::default(),
                    &Default::default(),
                    &Default::default()
                ),
                create_simple_config(
                    vec![Build::new(
                        vec!["foo".into()],
                        vec![],
                        Rule::new("touch foo", None).into(),
                        vec![],
                        vec!["bar".into()],
                        None,
                    )],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::FileNotFound("bar".into()))
        );
    }

    #[tokio::test]
    async fn report_source_of_missing_input() {
        let context = create_context(
            &Default::default(),
            &Default::default(),
            &Default::default(),
        );

        context.database().set_source("foo.o", "foo.c").unwrap();

        assert_eq!(
            run(
                &context,
                create_simple_config(
                    vec![explicit_build(
                        vec!["foo".into()],
                        Rule::new("cc foo.o", None),
                        vec!["foo.o".into()],
                    )],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::FileNotFound("foo.c".into()))
        );
    }

    #[tokio::test]
    async fn detect_circular_dependency() {
        assert_eq!(
            run(
                &create_context(
                    &Default::default(),
                    &Default::default(),
                    &Default::default()
                ),
                create_simple_config(
                    vec![explicit_build(
                        vec!["foo".into()],
                        Rule::new("cp foo foo", None),
                        vec!["foo".into()],
                    )],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::BuildGraph(BuildGraphError::CircularDependency(
                vec!["foo".into()]
            )))
        );
    }

    #[tokio::test]
    async fn report_sources_in_circular_dependency() {
        let context = create_context(
            &Default::default(),
            &Default::default(),
            &Default::default(),
        );

        context.database().set_source("foo", "baz").unwrap();
        context.database().set_source("bar", "baz").unwrap();

        assert_eq!(
            run(
                &context,
                create_simple_config(
                    vec![
                        explicit_build(
                            vec!["foo".into()],
                            Rule::new("cp bar foo", None),
                            vec!["bar".into()],
                        ),
                        explicit_build(
                            vec!["bar".into()],
                            Rule::new("cp foo bar", None),
                            vec!["foo".into()],
                        ),
                    ],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::BuildGraph(BuildGraphError::CircularDependency(
                vec!["baz".into()]
            )))
        );
    }

    #[tokio::test]
    async fn prepare_output_directory() {
        let context = create_context(
            &Default::default(),
            &Default::default(),
            &Default::default(),
        );

        run(
            &context,
            create_simple_config(
                vec![explicit_build(
                    vec!["foo/bar".into()],
                    Rule::new("touch foo/bar", None),
                    vec![],
                )],
                &["foo/bar"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert!(context.file_system().exists("foo".as_ref()).await.unwrap());
    }

    #[tokio::test]
    async fn record_output_with_source() {
        let file_system = FakeFileSystem::default();
        let context = create_context(&Default::default(), &Default::default(), &file_system);

        file_system.write_file("foo.c", "");

        run(
            &context,
            Config::new(
                create_outputs(vec![explicit_build(
                    vec!["foo.o".into()],
                    Rule::new("cc foo.c", None),
                    vec!["foo.c".into()],
                )]),
                ["foo.o".into()].into_iter().collect(),
                [("foo.o".into(), "foo.c".into())].into_iter().collect(),
                None,
            )
            .into(),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(context.database().get_outputs().unwrap(), ["foo.o"]);
        assert_eq!(
            context.database().get_source("foo.o").unwrap(),
            Some("foo.c".into())
        );
    }

    #[tokio::test]
    async fn skip_up_to_date_build() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let config = create_simple_config(
            vec![explicit_build(
                vec!["foo".into()],
                Rule::new("cp bar foo", None),
                vec!["bar".into()],
            )],
            &["foo"],
        );

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();
        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(command_runner.commands(), ["cp bar foo"]);
    }

    #[tokio::test]
    async fn skip_build_on_timestamp_update_of_input() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let config = create_simple_config(
            vec![explicit_build(
                vec!["foo".into()],
                Rule::new("cp bar foo", None),
                vec!["bar".into()],
            )],
            &["foo"],
        );

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();

        file_system.write_file("bar", "");

        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(command_runner.commands(), ["cp bar foo"]);
    }

    #[tokio::test]
    async fn rebuild_on_content_update_of_input() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let config = create_simple_config(
            vec![explicit_build(
                vec!["foo".into()],
                Rule::new("cp bar foo", None),
                vec!["bar".into()],
            )],
            &["foo"],
        );

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();

        file_system.write_file("bar", "bar");

        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(command_runner.commands(), ["cp bar foo", "cp bar foo"]);
    }

    #[tokio::test]
    async fn rebuild_on_update_of_phony_input() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let config = create_simple_config(
            vec![
                explicit_build(
                    vec!["foo".into()],
                    Rule::new("cp bar foo", None),
                    vec!["bar".into()],
                ),
                Build::new(
                    vec!["bar".into()],
                    vec![],
                    None,
                    vec!["baz".into()],
                    vec![],
                    None,
                ),
            ],
            &["foo"],
        );

        file_system.write_file("foo", "");
        file_system.write_file("baz", "");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();

        file_system.write_file("baz", "baz");

        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(command_runner.commands(), ["cp bar foo", "cp bar foo"]);
    }

    #[tokio::test]
    async fn rebuild_missing_output() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let config = create_simple_config(
            vec![explicit_build(
                vec!["foo".into()],
                Rule::new("cp bar foo", None),
                vec!["bar".into()],
            )],
            &["foo"],
        );

        file_system.write_file("bar", "");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();
        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(command_runner.commands(), ["cp bar foo", "cp bar foo"]);
    }

    #[tokio::test]
    async fn rebuild_missing_implicit_output() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let config = create_simple_config(
            vec![Build::new(
                vec!["foo".into()],
                vec!["baz".into()],
                Rule::new("cp bar foo", None).into(),
                vec!["bar".into()],
                vec![],
                None,
            )],
            &["foo"],
        );

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();
        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(command_runner.commands(), ["cp bar foo", "cp bar foo"]);
    }

    #[tokio::test]
    async fn rerun_failed_build() {
        let command_runner =
            FakeCommandRunner::new([("exit 1".into(), failed_output())].into_iter().collect());
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let config = create_simple_config(
            vec![explicit_build(
                vec!["foo".into()],
                Rule::new("exit 1", None),
                vec![],
            )],
            &["foo"],
        );

        file_system.write_file("foo", "");

        assert_eq!(
            run(&context, config.clone(), &[], DEFAULT_OPTIONS).await,
            Err(BuildError::Build)
        );
        assert_eq!(
            run(&context, config, &[], DEFAULT_OPTIONS).await,
            Err(BuildError::Build)
        );
        assert_eq!(command_runner.commands(), ["exit 1", "exit 1"]);
    }

    #[tokio::test]
    async fn write_command_output() {
        let console = FakeConsole::default();

        run(
            &create_context(
                &FakeCommandRunner::new(
                    [(
                        "touch foo".into(),
                        Output {
                            status: ExitStatus::default(),
                            stdout: b"bar\n".into(),
                            stderr: b"baz\n".into(),
                        },
                    )]
                    .into_iter()
                    .collect(),
                ),
                &console,
                &Default::default(),
            ),
            create_simple_config(
                vec![explicit_build(
                    vec!["foo".into()],
                    Rule::new("touch foo", Some("build foo".into())),
                    vec![],
                )],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(console.stdout(), "bar\n");
        assert_eq!(console.stderr(), "build foo\nbaz\n");
    }

    #[tokio::test]
    async fn write_debug_log_of_failed_build() {
        let console = FakeConsole::default();

        assert_eq!(
            run(
                &create_context(
                    &FakeCommandRunner::new(
                        [("exit 1".into(), failed_output())].into_iter().collect(),
                    ),
                    &console,
                    &Default::default(),
                ),
                create_simple_config(
                    vec![explicit_build(
                        vec!["foo".into()],
                        Rule::new("exit 1", None),
                        vec![],
                    )],
                    &["foo"],
                ),
                &[],
                RunOptions {
                    debug: true,
                    profile: false,
                },
            )
            .await,
            Err(BuildError::Build)
        );
        assert_eq!(
            console.stderr(),
            "turtle: command: exit 1\nturtle: exit status: 1\n"
        );
    }

    #[tokio::test]
    async fn write_profile_log() {
        let console = FakeConsole::default();

        run(
            &create_context(&Default::default(), &console, &Default::default()),
            create_simple_config(
                vec![explicit_build(
                    vec!["foo".into()],
                    Rule::new("touch foo", None),
                    vec![],
                )],
                &["foo"],
            ),
            &[],
            RunOptions {
                debug: false,
                profile: true,
            },
        )
        .await
        .unwrap();

        assert!(
            Regex::new(r"^turtle: duration: \d+ ms\n$")
                .unwrap()
                .is_match(&console.stderr())
        );
    }

    #[tokio::test]
    async fn rebuild_on_update_of_header_dependency() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let config = create_simple_config(
            vec![explicit_build(
                vec!["foo.o".into()],
                Rule::new("cc foo.c", None).with_header_dependency(Some(HeaderDependency::Make {
                    path: "foo.d".into(),
                })),
                vec!["foo.c".into()],
            )],
            &["foo.o"],
        );

        file_system.write_file("foo.o", "");
        file_system.write_file("foo.c", "");
        file_system.write_file("foo.h", "");
        file_system.write_file("foo.d", "foo.o: foo.h\n");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();
        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();

        file_system.write_file("foo.h", "foo");

        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(command_runner.commands(), ["cc foo.c", "cc foo.c"]);
    }

    #[tokio::test]
    async fn read_and_remove_gcc_depfile() {
        let file_system = FakeFileSystem::default();
        let context = create_context(&Default::default(), &Default::default(), &file_system);
        let build = explicit_build(
            vec!["foo.o".into()],
            Rule::new("cc foo.c", None).with_header_dependency(Some(HeaderDependency::Gcc {
                path: "foo.d".into(),
            })),
            vec!["foo.c".into()],
        );

        file_system.write_file("foo.c", "");
        file_system.write_file("foo.h", "");
        file_system.write_file("foo.d", "foo.o: foo.h\n");

        run(
            &context,
            create_simple_config(vec![build.clone()], &["foo.o"]),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(
            context
                .database()
                .get_header_dependencies(build.id())
                .unwrap(),
            ["foo.h"]
        );
        assert!(
            !context
                .file_system()
                .exists("foo.d".as_ref())
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn read_msvc_header_dependencies() {
        let console = FakeConsole::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(
            &FakeCommandRunner::new(
                [(
                    "cl foo.c".into(),
                    Output {
                        status: ExitStatus::default(),
                        stdout: b"Note: including file: foo.h\nfoo.c\n".into(),
                        stderr: vec![],
                    },
                )]
                .into_iter()
                .collect(),
            ),
            &console,
            &file_system,
        );
        let build = explicit_build(
            vec!["foo.obj".into()],
            Rule::new("cl foo.c", None).with_header_dependency(Some(HeaderDependency::Msvc {
                prefix: "Note: including file: ".into(),
            })),
            vec!["foo.c".into()],
        );

        file_system.write_file("foo.c", "");
        file_system.write_file("foo.h", "");

        run(
            &context,
            create_simple_config(vec![build.clone()], &["foo.obj"]),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(
            context
                .database()
                .get_header_dependencies(build.id())
                .unwrap(),
            ["foo.h"]
        );
        assert_eq!(console.stdout(), "foo.c\n");
    }

    #[tokio::test]
    async fn build_generated_header_dependency() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let build = explicit_build(
            vec!["foo.o".into()],
            Rule::new("cc foo.c", None),
            vec!["foo.c".into()],
        );

        context
            .database()
            .set_header_dependencies(build.id(), &["foo.h".into()])
            .unwrap();
        file_system.write_file("foo.c", "");
        file_system.write_file("foo.h", "");

        run(
            &context,
            create_simple_config(
                vec![
                    build,
                    explicit_build(vec!["foo.h".into()], Rule::new("touch foo.h", None), vec![]),
                ],
                &["foo.o"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands(), ["touch foo.h", "cc foo.c"]);
    }

    #[tokio::test]
    async fn ignore_missing_header_dependency() {
        let file_system = FakeFileSystem::default();
        let context = create_context(&Default::default(), &Default::default(), &file_system);
        let build = explicit_build(
            vec!["foo.o".into()],
            Rule::new("cc foo.c", None),
            vec!["foo.c".into()],
        );

        context
            .database()
            .set_header_dependencies(build.id(), &["foo.h".into()])
            .unwrap();
        file_system.write_file("foo.c", "");

        run(
            &context,
            create_simple_config(vec![build], &["foo.o"]),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn detect_circular_header_dependency() {
        let context = create_context(
            &Default::default(),
            &Default::default(),
            &Default::default(),
        );
        let build = explicit_build(vec!["foo".into()], Rule::new("touch foo", None), vec![]);

        context
            .database()
            .set_header_dependencies(build.id(), &["foo".into()])
            .unwrap();

        assert_eq!(
            run(
                &context,
                create_simple_config(vec![build], &["foo"]),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::BuildGraph(BuildGraphError::CircularDependency(
                vec!["foo".into()]
            )))
        );
    }

    #[tokio::test]
    async fn build_dynamic_input() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();

        file_system.write_file(
            "foo.dd",
            "ninja_dyndep_version = 1\nbuild foo: dyndep | bar\n",
        );
        file_system.write_file("bar", "");

        run(
            &create_context(&command_runner, &Default::default(), &file_system),
            create_simple_config(
                vec![
                    Build::new(
                        vec!["foo".into()],
                        vec![],
                        Rule::new("touch foo", None).into(),
                        vec![],
                        vec!["foo.dd".into()],
                        Some("foo.dd".into()),
                    ),
                    explicit_build(
                        vec!["foo.dd".into()],
                        Rule::new("touch foo.dd", None),
                        vec![],
                    ),
                    explicit_build(vec!["bar".into()], Rule::new("touch bar", None), vec![]),
                ],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(
            command_runner.commands(),
            ["touch foo.dd", "touch bar", "touch foo"]
        );
    }

    #[tokio::test]
    async fn build_dynamic_input_declared_for_implicit_output() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();

        file_system.write_file(
            "foo.dd",
            "ninja_dyndep_version = 1\nbuild baz: dyndep | bar\n",
        );
        file_system.write_file("bar", "");

        run(
            &create_context(&command_runner, &Default::default(), &file_system),
            create_simple_config(
                vec![
                    Build::new(
                        vec!["foo".into()],
                        vec!["baz".into()],
                        Rule::new("touch foo", None).into(),
                        vec![],
                        vec!["foo.dd".into()],
                        Some("foo.dd".into()),
                    ),
                    explicit_build(
                        vec!["foo.dd".into()],
                        Rule::new("touch foo.dd", None),
                        vec![],
                    ),
                    explicit_build(vec!["bar".into()], Rule::new("touch bar", None), vec![]),
                ],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(
            command_runner.commands(),
            ["touch foo.dd", "touch bar", "touch foo"]
        );
    }

    #[tokio::test]
    async fn fail_with_missing_dynamic_dependency() {
        let file_system = FakeFileSystem::default();
        let build = Build::new(
            vec!["foo".into()],
            vec![],
            Rule::new("touch foo", None).into(),
            vec![],
            vec![],
            Some("foo.dd".into()),
        );

        file_system.write_file("foo.dd", "ninja_dyndep_version = 1\n");

        assert_eq!(
            run(
                &create_context(&Default::default(), &Default::default(), &file_system),
                create_simple_config(vec![build.clone()], &["foo"]),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::DynamicDependencyNotFound(build.into()))
        );
    }

    #[tokio::test]
    async fn fail_with_unknown_output_in_dynamic_dependency() {
        let file_system = FakeFileSystem::default();

        file_system.write_file(
            "foo.dd",
            "ninja_dyndep_version = 1\nbuild bar: dyndep | baz\n",
        );

        assert_eq!(
            run(
                &create_context(&Default::default(), &Default::default(), &file_system),
                create_simple_config(
                    vec![Build::new(
                        vec!["foo".into()],
                        vec![],
                        Rule::new("touch foo", None).into(),
                        vec![],
                        vec![],
                        Some("foo.dd".into()),
                    )],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::BuildGraph(BuildGraphError::OutputNotFound(
                "bar".into()
            )))
        );
    }

    #[tokio::test]
    async fn detect_circular_dynamic_dependency() {
        let file_system = FakeFileSystem::default();

        file_system.write_file(
            "foo.dd",
            "ninja_dyndep_version = 1\nbuild foo: dyndep | foo\n",
        );

        assert_eq!(
            run(
                &create_context(&Default::default(), &Default::default(), &file_system),
                create_simple_config(
                    vec![Build::new(
                        vec!["foo".into()],
                        vec![],
                        Rule::new("touch foo", None).into(),
                        vec![],
                        vec![],
                        Some("foo.dd".into()),
                    )],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::BuildGraph(BuildGraphError::CircularDependency(
                vec!["foo".into()]
            )))
        );
    }

    #[test]
    fn classify_phony_and_file_inputs() {
        let build = explicit_build(
            vec!["foo".into()],
            Rule::new("", None),
            vec!["bar".into(), "baz".into(), "qux".into(), "bar".into()],
        );
        let context = RunContext::new(
            create_context(
                &Default::default(),
                &Default::default(),
                &Default::default(),
            ),
            create_simple_config(
                vec![
                    build.clone(),
                    Build::new(vec!["bar".into()], vec![], None, vec![], vec![], None),
                    explicit_build(vec!["baz".into()], Rule::new("", None), vec![]),
                ],
                &[],
            ),
            BuildGraph::new(&Default::default()),
            DEFAULT_OPTIONS,
        );

        assert_eq!(
            classify_inputs(
                &context,
                &build,
                &["quux".into(), "baz".into()],
                &["qux".into(), "corge".into()],
            ),
            (vec!["bar"], vec!["baz", "qux", "quux", "corge"])
        );
    }

    mod file_state {
        use super::*;
        use pretty_assertions::assert_eq;

        fn count_requests(requests: &[PathBuf], path: &str) -> usize {
            requests
                .iter()
                .filter(|request| request.as_path() == Path::new(path))
                .count()
        }

        fn create_chain_config() -> Arc<Config> {
            create_simple_config(
                vec![
                    explicit_build(
                        vec!["foo".into()],
                        Rule::new("cp bar foo", None),
                        vec!["bar".into()],
                    ),
                    explicit_build(vec!["bar".into()], Rule::new("touch bar", None), vec![]),
                ],
                &["foo"],
            )
        }

        fn create_run_context(context: Arc<Context>) -> Arc<RunContext> {
            RunContext::new(
                context,
                create_simple_config(vec![], &[]),
                BuildGraph::new(&Default::default()),
                DEFAULT_OPTIONS,
            )
            .into()
        }

        #[tokio::test]
        async fn look_up_shared_input_once() {
            let file_system = FakeFileSystem::default();

            file_system.write_file("baz", "");

            run(
                &create_context(&Default::default(), &Default::default(), &file_system),
                create_simple_config(
                    vec![
                        explicit_build(
                            vec!["foo".into()],
                            Rule::new("cp baz foo", None),
                            vec!["baz".into()],
                        ),
                        explicit_build(
                            vec!["bar".into()],
                            Rule::new("cp baz bar", None),
                            vec!["baz".into()],
                        ),
                    ],
                    &["foo", "bar"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await
            .unwrap();

            assert_eq!(
                count_requests(&file_system.modified_time_requests(), "baz"),
                1
            );
            assert_eq!(count_requests(&file_system.content_requests(), "baz"), 1);
        }

        #[tokio::test]
        async fn look_up_shared_header_dependency_once() {
            let file_system = FakeFileSystem::default();
            let context = create_context(&Default::default(), &Default::default(), &file_system);
            let builds = vec![
                explicit_build(
                    vec!["foo.o".into()],
                    Rule::new("cc foo.c", None),
                    vec!["foo.c".into()],
                ),
                explicit_build(
                    vec!["bar.o".into()],
                    Rule::new("cc bar.c", None),
                    vec!["bar.c".into()],
                ),
            ];

            for build in &builds {
                context
                    .database()
                    .set_header_dependencies(build.id(), &["foo.h".into()])
                    .unwrap();
            }

            file_system.write_file("foo.c", "");
            file_system.write_file("bar.c", "");
            file_system.write_file("foo.h", "");

            run(
                &context,
                create_simple_config(builds, &["foo.o", "bar.o"]),
                &[],
                DEFAULT_OPTIONS,
            )
            .await
            .unwrap();

            assert_eq!(
                count_requests(&file_system.modified_time_requests(), "foo.h"),
                1
            );
        }

        #[tokio::test]
        async fn look_up_output_again_after_rule() {
            let file_system = FakeFileSystem::default();

            file_system.write_file("bar", "");

            run(
                &create_context(&Default::default(), &Default::default(), &file_system),
                create_chain_config(),
                &[],
                DEFAULT_OPTIONS,
            )
            .await
            .unwrap();

            assert_eq!(
                count_requests(&file_system.modified_time_requests(), "bar"),
                2
            );
        }

        #[tokio::test]
        async fn look_up_output_again_after_failed_rule() {
            let file_system = FakeFileSystem::default();
            let context = create_run_context(create_context(
                &FakeCommandRunner::new([("exit 1".into(), failed_output())].into_iter().collect()),
                &Default::default(),
                &file_system,
            ));

            file_system.write_file("foo", "");
            get_modified_times(&context, &["foo"]).await;

            assert_eq!(
                run_rule_and_invalidate_outputs(&context, &Rule::new("exit 1", None), &["foo"])
                    .await,
                Err(BuildError::Build)
            );

            get_modified_times(&context, &["foo"]).await;

            assert_eq!(
                count_requests(&file_system.modified_time_requests(), "foo"),
                2
            );
        }

        #[tokio::test]
        async fn look_up_output_of_phony_build_again_after_inputs() {
            let file_system = FakeFileSystem::default();
            let context = create_context(&Default::default(), &Default::default(), &file_system);
            let build = explicit_build(vec!["foo".into()], Rule::new("touch foo", None), vec![]);

            context
                .database()
                .set_header_dependencies(build.id(), &["bar".into()])
                .unwrap();

            file_system.write_file("bar", "");
            file_system.write_file("baz", "");

            run(
                &context,
                create_simple_config(
                    vec![
                        build,
                        Build::new(
                            vec!["bar".into()],
                            vec![],
                            None,
                            vec!["baz".into()],
                            vec![],
                            None,
                        ),
                        explicit_build(vec!["baz".into()], Rule::new("touch baz", None), vec![]),
                    ],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await
            .unwrap();

            assert_eq!(
                count_requests(&file_system.modified_time_requests(), "bar"),
                2
            );
        }

        #[tokio::test]
        async fn reuse_output_state_of_up_to_date_build() {
            let command_runner = FakeCommandRunner::default();
            let file_system = FakeFileSystem::default();
            let context = create_context(&command_runner, &Default::default(), &file_system);

            file_system.write_file("foo", "");
            file_system.write_file("bar", "");

            run(&context, create_chain_config(), &[], DEFAULT_OPTIONS)
                .await
                .unwrap();

            let count = count_requests(&file_system.modified_time_requests(), "bar");

            run(&context, create_chain_config(), &[], DEFAULT_OPTIONS)
                .await
                .unwrap();

            assert_eq!(command_runner.commands(), ["touch bar", "cp bar foo"]);
            assert_eq!(
                count_requests(&file_system.modified_time_requests(), "bar") - count,
                1
            );
        }

        #[tokio::test]
        async fn fail_with_missing_output_of_rule() {
            assert_eq!(
                run(
                    &create_context(
                        &Default::default(),
                        &Default::default(),
                        &Default::default()
                    ),
                    create_chain_config(),
                    &[],
                    DEFAULT_OPTIONS,
                )
                .await,
                Err(BuildError::FileNotFound("bar".into()))
            );
        }

        #[tokio::test]
        async fn skip_directory_creation_for_top_level_output() {
            let context = create_context(
                &Default::default(),
                &Default::default(),
                &Default::default(),
            );

            run(
                &context,
                create_simple_config(
                    vec![explicit_build(
                        vec!["foo".into()],
                        Rule::new("touch foo", None),
                        vec![],
                    )],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await
            .unwrap();

            assert!(!context.file_system().exists("".as_ref()).await.unwrap());
        }

        #[test]
        fn drop_missing_header_dependency() {
            let time = |seconds| SystemTime::UNIX_EPOCH + core::time::Duration::from_secs(seconds);

            assert_eq!(
                retain_existing_files(
                    [
                        ("foo.c", (time(1), Some(1))),
                        ("foo.h", (time(2), None)),
                        ("bar.h", (time(3), Some(3))),
                    ],
                    &["foo.c"],
                ),
                Ok(vec![(time(1), 1), (time(3), 3)])
            );
        }

        #[test]
        fn fail_with_missing_explicit_input() {
            assert_eq!(
                retain_existing_files(
                    [
                        ("foo.h", (SystemTime::UNIX_EPOCH, None)),
                        ("foo.c", (SystemTime::UNIX_EPOCH, None)),
                    ],
                    &["foo.c"],
                ),
                Err("foo.c")
            );
        }

        #[test]
        fn drop_missing_header_dependency_from_hashes() {
            let context = create_run_context(create_context(
                &Default::default(),
                &Default::default(),
                &Default::default(),
            ));
            let build = explicit_build(vec!["foo".into()], Rule::new("", None), vec![]);

            assert_eq!(
                calculate_hashes(
                    &context,
                    &build,
                    &[],
                    &["bar"],
                    vec![SystemTime::UNIX_EPOCH],
                    vec![Ok(None)],
                    &[],
                ),
                calculate_hashes(&context, &build, &[], &[], vec![], vec![], &[])
            );
        }

        #[test]
        fn fail_with_missing_dynamic_input() {
            assert_eq!(
                calculate_hashes(
                    &create_run_context(create_context(
                        &Default::default(),
                        &Default::default(),
                        &Default::default(),
                    )),
                    &explicit_build(vec!["foo".into()], Rule::new("", None), vec![]),
                    &["bar".into()],
                    &["bar"],
                    vec![SystemTime::UNIX_EPOCH],
                    vec![Ok(None)],
                    &[],
                ),
                Err(BuildError::FileNotFound("bar".into()))
            );
        }
    }
}

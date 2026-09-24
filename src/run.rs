mod context;
mod file_cache;
mod hash;
mod header_dependency;
mod log;
mod options;

use self::{
    context::RunContext,
    hash::{calculate_content_hash, calculate_timestamp_hash},
    header_dependency::{exclude_show_includes, read_header_dependencies},
    log::{debug, profile},
};
use crate::{
    build_graph::{BuildGraph, BuildGraphError},
    compile::compile_dynamic,
    context::Context,
    error::BuildError,
    hash_type::HashType,
    infrastructure::{ConsolePermit, Metadata},
    ir::{Build, Config, DynamicConfig, Pool, Rule},
    parse::parse_dynamic,
};
use alloc::sync::Arc;
use async_recursion::async_recursion;
use futures::future::{FutureExt, try_join_all};
use itertools::Itertools;
pub use options::RunOptions;
use std::{collections::HashMap, path::Path, process::Output};
use tokio::{spawn, time::Instant, try_join};

/// Runs builds.
pub async fn run(
    context: &Arc<Context>,
    config: Arc<Config>,
    outputs: &[String],
    options: RunOptions,
) -> Result<(), BuildError> {
    let mut graph = BuildGraph::new(config.outputs());
    let mut header_dependencies = HashMap::new();

    for build in config
        .outputs()
        .values()
        .filter(|build| build.rule().is_some())
        .unique_by(|build| build.id())
    {
        let dependencies = context.database().get_header_dependencies(build.id())?;

        graph.add_header_dependencies(&build.outputs()[0], &dependencies);
        header_dependencies.insert(build.id(), dependencies);
    }

    let context = Arc::new(RunContext::new(
        context.clone(),
        config,
        graph,
        header_dependencies,
        options,
    ));

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

    // Write outputs queued during builds, including a failed one, before the process exits.
    context.console().lock().await?.flush().await?;

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
        let (_, output_metadata) = try_join!(
            try_join_all(
                build
                    .inputs()
                    .iter()
                    .chain(build.order_only_inputs())
                    .map(|input| build_input(context.clone(), input)),
            ),
            get_output_metadata(&context, &build),
        )?;

        let dynamic_inputs = if let Some(path) = build.dynamic_module() {
            let config = load_dynamic_config(&context, path).await?;

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

        try_join_all(
            dynamic_inputs
                .iter()
                .map(|input| build_input(context.clone(), input)),
        )
        .await?;

        if build.rule().is_none() {
            // TODO Consider dropping this case by assuming that outputs of phony
            // rules are always virtual.
            invalidate_outputs(&context, &build).await;
        }

        let dependencies = context.header_dependencies(build.id());

        try_join_all(
            dependencies
                .iter()
                .filter_map(|dependency| context.config().outputs().get(dependency))
                .map(|build| run_build(context.clone(), build)),
        )
        .await?;

        let header_dependencies =
            filter_existing_header_dependencies(&context, dependencies).await?;

        let (phony_inputs, file_inputs) =
            classify_inputs(&context, &build, dynamic_inputs, &header_dependencies);
        let mut timestamp_hash =
            calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        if let Some(metadata) = &output_metadata
            && Some(timestamp_hash)
                == context
                    .build()
                    .database()
                    .get_hash(HashType::Timestamp, build.id())?
        {
            cache_output_metadata(&context, &build, metadata).await;

            return Ok(());
        }

        let mut content_hash =
            calculate_content_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        if output_metadata.is_some()
            && Some(content_hash)
                == context
                    .build()
                    .database()
                    .get_hash(HashType::Content, build.id())?
        {
            return Ok(());
        } else if let Some(rule) = build.rule() {
            try_join_all(
                build
                    .outputs()
                    .iter()
                    .chain(build.implicit_outputs())
                    .map(|path| prepare_directory(&context, path.as_ref())),
            )
            .await?;

            let output = run_rule(&context, rule).await;

            invalidate_outputs(&context, &build).await;

            let new_header_dependencies =
                read_header_dependencies(&context, rule, &output?).await?;

            context
                .build()
                .database()
                .set_header_dependencies(build.id(), &new_header_dependencies)?;

            for output in build.outputs() {
                context.build().database().set_output(output)?;

                if let Some(source) = context.config().source_map().get(output) {
                    context.build().database().set_source(output, source)?;
                }
            }

            if header_dependencies
                .iter()
                .copied()
                .ne(&new_header_dependencies)
            {
                let header_dependencies =
                    filter_existing_header_dependencies(&context, &new_header_dependencies).await?;
                let (phony_inputs, file_inputs) =
                    classify_inputs(&context, &build, dynamic_inputs, &header_dependencies);

                timestamp_hash =
                    calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs).await?;
                content_hash =
                    calculate_content_hash(&context, &build, &file_inputs, &phony_inputs).await?;
            }
        }

        context
            .build()
            .database()
            .set_hash(HashType::Timestamp, build.id(), timestamp_hash)?;
        context
            .build()
            .database()
            .set_hash(HashType::Content, build.id(), content_hash)?;

        Ok(())
    })
    .await?
}

async fn build_input(context: Arc<RunContext>, input: &Arc<str>) -> Result<(), BuildError> {
    if let Some(build) = context.config().outputs().get(input) {
        run_build(context.clone(), build).await
    } else {
        check_file_existence(&context, input).await
    }
}

async fn load_dynamic_config<'a>(
    context: &'a RunContext,
    path: &str,
) -> Result<&'a DynamicConfig, BuildError> {
    context
        .dynamic_config(path)
        .get_or_try_init(|| async {
            let config = compile_dynamic(
                &parse_dynamic(
                    &context
                        .build()
                        .file_system()
                        .read_file_to_string(path.as_ref())
                        .await?,
                )?,
                context.build().path_pool(),
            )?;

            context
                .build_graph()
                .lock()
                .await
                .validate_dynamic(&config)
                .map_err(|error| map_build_graph_error(context, &error))?;

            Ok(config)
        })
        .await
}

async fn get_output_metadata(
    context: &RunContext,
    build: &Build,
) -> Result<Option<Vec<Metadata>>, BuildError> {
    Ok(try_join_all(
        build
            .outputs()
            .iter()
            .chain(build.implicit_outputs())
            .map(|path| {
                context
                    .build()
                    .file_system()
                    .metadata(path.as_ref().as_ref())
            }),
    )
    .await?
    .into_iter()
    .collect())
}

async fn cache_output_metadata(context: &RunContext, build: &Build, metadata: &[Metadata]) {
    // Inputs of phony builds might write their outputs.
    if build.rule().is_none() {
        return;
    }

    for (path, &metadata) in build
        .outputs()
        .iter()
        .chain(build.implicit_outputs())
        .zip(metadata)
    {
        context.file_cache().set_metadata(path, metadata).await;
    }
}

async fn filter_existing_header_dependencies<'a>(
    context: &RunContext,
    dependencies: &'a [Arc<str>],
) -> Result<Vec<&'a Arc<str>>, BuildError> {
    let mut existing_dependencies = vec![];

    for dependency in dependencies {
        if context.file_cache().exists(dependency).await? {
            existing_dependencies.push(dependency);
        }
    }

    Ok(existing_dependencies)
}

async fn check_file_existence(context: &RunContext, path: &Arc<str>) -> Result<(), BuildError> {
    if !context.file_cache().exists(path).await? {
        return Err(BuildError::FileNotFound(
            context
                .build()
                .database()
                .get_source(path)?
                .unwrap_or_else(|| path.as_ref().into()),
        ));
    }

    Ok(())
}

async fn prepare_directory(context: &RunContext, path: impl AsRef<Path>) -> Result<(), BuildError> {
    if let Some(directory) = path.as_ref().parent() {
        context
            .build()
            .file_system()
            .create_directory(directory)
            .await?;
    }

    Ok(())
}

async fn invalidate_outputs(context: &RunContext, build: &Build) {
    for output in build.outputs().iter().chain(build.implicit_outputs()) {
        context.file_cache().invalidate(output).await;
    }
}

fn classify_inputs<'a>(
    context: &'a RunContext,
    build: &'a Build,
    dynamic_inputs: &'a [Arc<str>],
    header_dependencies: &'a [&'a Arc<str>],
) -> (Vec<&'a Arc<str>>, Vec<&'a Arc<str>>) {
    let (phony_inputs, file_inputs) = build
        .inputs()
        .iter()
        .chain(dynamic_inputs)
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
            .chain(header_dependencies.iter().copied())
            .unique()
            .collect(),
    )
}

async fn run_rule(context: &RunContext, rule: &Rule) -> Result<Output, BuildError> {
    let (mut console, output, duration) = if rule.pool() == Some(&Pool::Console) {
        let mut console = context.console().lock().await?;

        write_description(context, rule, &mut console).await?;
        console.flush().await?;

        let time = Instant::now();
        let status = context
            .build()
            .command_runner()
            .run_with_console(rule.command())
            .await?;

        (
            console,
            Output {
                status,
                stdout: vec![],
                stderr: vec![],
            },
            Instant::now() - time,
        )
    } else {
        let permit = context.pool(rule.pool()).await?;
        let mut console = context.console().queue();

        write_description(context, rule, &mut console).await?;
        console.flush().await?;

        let time = Instant::now();
        let output = context.build().command_runner().run(rule.command()).await?;

        drop(permit);

        (console, output, Instant::now() - time)
    };

    profile!(context, console, "duration: {} ms", duration.as_millis());

    console
        .write_stdout(&exclude_show_includes(rule, &output.stdout))
        .await?;
    console.write_stderr(&output.stderr).await?;

    let result = if output.status.success() {
        Ok(output)
    } else {
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

        Err(BuildError::Build)
    };

    console.flush().await?;

    result
}

async fn write_description(
    context: &RunContext,
    rule: &Rule,
    console: &mut ConsolePermit<'_>,
) -> Result<(), BuildError> {
    if let Some(description) = rule.description() {
        console.write_stderr(description.as_bytes()).await?;
        console.write_stderr(b"\n").await?;
    }

    debug!(context, console, "command: {}", rule.command());

    Ok(())
}

fn map_build_graph_error(context: &RunContext, error: &BuildGraphError) -> BuildError {
    match error {
        BuildGraphError::CircularDependency(outputs) => {
            match outputs
                .iter()
                .map(|output| {
                    Ok(context
                        .build()
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
        infrastructure::{FakeCommandRunner, FakeConsole, FakeDatabase, FakeFileSystem, FileError},
        ir::HeaderDependency,
    };
    use core::{num::NonZeroUsize, pin::pin};
    use futures::poll;
    use pretty_assertions::{assert_eq, assert_ne};
    use regex::Regex;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    use std::{collections::HashMap, process::ExitStatus};
    use tokio::{sync::Mutex, task::yield_now};

    const DEFAULT_OPTIONS: RunOptions = RunOptions {
        debug: false,
        profile: false,
    };
    const POLL_COUNT: usize = 8;

    fn create_context(
        command_runner: &FakeCommandRunner,
        console: &FakeConsole,
        file_system: &FakeFileSystem,
    ) -> Arc<Context> {
        Context::new(
            command_runner.clone(),
            Mutex::new(console.clone()).into(),
            FakeDatabase::default(),
            file_system.clone(),
            Default::default(),
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
        create_pool_config(builds, default_outputs, &[])
    }

    fn create_pool_config(
        builds: Vec<Build>,
        default_outputs: &[&str],
        pools: &[(&str, usize)],
    ) -> Arc<Config> {
        Config::new(
            create_outputs(builds),
            default_outputs
                .iter()
                .map(|&output| output.into())
                .collect(),
            Default::default(),
            pools
                .iter()
                .map(|&(name, depth)| (name.into(), NonZeroUsize::new(depth).unwrap()))
                .collect(),
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

    fn limited_pool(name: &str) -> Option<Pool> {
        Some(Pool::Limited(name.into()))
    }

    fn pool_build(output: &str, pool: Option<Pool>) -> Build {
        explicit_build(
            vec![output.into()],
            Rule::new(format!("touch {output}"), None).with_pool(pool),
            vec![],
        )
    }

    fn count_metadata_requests(file_system: &FakeFileSystem, path: &str) -> usize {
        file_system
            .metadata_requests()
            .iter()
            .filter(|request| request.as_path() == Path::new(path))
            .count()
    }

    fn create_run_context(
        command_runner: &FakeCommandRunner,
        console: &FakeConsole,
        pools: &[(&str, usize)],
    ) -> RunContext {
        RunContext::new(
            create_context(command_runner, console, &Default::default()),
            create_pool_config(vec![], &[], pools),
            BuildGraph::new(&Default::default()),
            Default::default(),
            DEFAULT_OPTIONS,
        )
    }

    fn create_build_run_context(
        command_runner: &FakeCommandRunner,
        file_system: &FakeFileSystem,
        builds: Vec<Build>,
    ) -> Arc<RunContext> {
        RunContext::new(
            create_context(command_runner, &Default::default(), file_system),
            create_simple_config(builds, &[]),
            BuildGraph::new(&Default::default()),
            Default::default(),
            DEFAULT_OPTIONS,
        )
        .into()
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
                    explicit_build(
                        vec!["foo".into()],
                        Rule::new("touch foo".into(), None),
                        vec![],
                    ),
                    explicit_build(
                        vec!["bar".into()],
                        Rule::new("touch bar".into(), None),
                        vec![],
                    ),
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
                    explicit_build(
                        vec!["foo".into()],
                        Rule::new("touch foo".into(), None),
                        vec![],
                    ),
                    explicit_build(
                        vec!["bar".into()],
                        Rule::new("touch bar".into(), None),
                        vec![],
                    ),
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
                    Rule::new("touch foo".into(), None),
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
                        Rule::new("touch foo".into(), None),
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
                        Rule::new("touch foo".into(), None),
                        vec!["bar".into(), "baz".into()],
                    ),
                    explicit_build(
                        vec!["bar".into()],
                        Rule::new("touch bar".into(), None),
                        vec!["baz".into()],
                    ),
                    explicit_build(
                        vec!["baz".into()],
                        Rule::new("touch baz".into(), None),
                        vec![],
                    ),
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
                        Rule::new("touch foo".into(), None).into(),
                        vec![],
                        vec!["bar".into()],
                        None,
                    ),
                    explicit_build(
                        vec!["bar".into()],
                        Rule::new("touch bar".into(), None),
                        vec![],
                    ),
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
                        Rule::new("touch foo bar".into(), None),
                        vec![],
                    ),
                    explicit_build(
                        vec!["baz".into()],
                        Rule::new("cat foo bar > baz".into(), None),
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
                        Rule::new("cp bar foo".into(), None),
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
                        Rule::new("touch foo".into(), None).into(),
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
                        Rule::new("cc foo.o".into(), None),
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
                        Rule::new("cp foo foo".into(), None),
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
                            Rule::new("cp bar foo".into(), None),
                            vec!["bar".into()],
                        ),
                        explicit_build(
                            vec!["bar".into()],
                            Rule::new("cp foo bar".into(), None),
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
                    Rule::new("touch foo/bar".into(), None),
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
                    Rule::new("cc foo.c".into(), None),
                    vec!["foo.c".into()],
                )]),
                ["foo.o".into()].into_iter().collect(),
                [("foo.o".into(), "foo.c".into())].into_iter().collect(),
                Default::default(),
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
                Rule::new("cp bar foo".into(), None),
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
                Rule::new("cp bar foo".into(), None),
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
                Rule::new("cp bar foo".into(), None),
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
                    Rule::new("cp bar foo".into(), None),
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
                Rule::new("cp bar foo".into(), None),
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
                Rule::new("cp bar foo".into(), None).into(),
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
                Rule::new("exit 1".into(), None),
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
    async fn invalidate_file_cache_of_outputs_after_command() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_build_run_context(&command_runner, &file_system, vec![]);
        let build = Build::new(
            vec!["foo".into()],
            vec!["bar".into()],
            Rule::new("touch foo bar".into(), None).into(),
            vec![],
            vec![],
            None,
        )
        .into();
        let mut future = pin!(run_build(context.clone(), &build));

        file_system.write_file("foo", "1");
        file_system.write_file("bar", "1");

        assert!(poll!(&mut future).is_pending());

        // Let the runtime run the build until its command suspends.
        yield_now().await;

        assert_eq!(command_runner.commands(), ["touch foo bar"]);

        let foo_hash = context.file_cache().content_hash(&"foo".into()).await;
        let bar_hash = context.file_cache().content_hash(&"bar".into()).await;

        file_system.write_file("foo", "2");
        file_system.write_file("bar", "2");
        future.await.unwrap();

        assert_ne!(
            context.file_cache().content_hash(&"foo".into()).await,
            foo_hash
        );
        assert_ne!(
            context.file_cache().content_hash(&"bar".into()).await,
            bar_hash
        );
    }

    #[tokio::test]
    async fn invalidate_file_cache_of_output_after_failed_command() {
        let command_runner =
            FakeCommandRunner::new([("exit 1".into(), failed_output())].into_iter().collect());
        let file_system = FakeFileSystem::default();
        let context = create_build_run_context(&command_runner, &file_system, vec![]);
        let build =
            explicit_build(vec!["foo".into()], Rule::new("exit 1".into(), None), vec![]).into();
        let mut future = pin!(run_build(context.clone(), &build));

        file_system.write_file("foo", "1");

        assert!(poll!(&mut future).is_pending());

        // Let the runtime run the build until its command suspends.
        yield_now().await;

        assert_eq!(command_runner.commands(), ["exit 1"]);

        let hash = context.file_cache().content_hash(&"foo".into()).await;

        file_system.write_file("foo", "2");

        assert_eq!(future.await, Err(BuildError::Build));
        assert_ne!(context.file_cache().content_hash(&"foo".into()).await, hash);
    }

    #[tokio::test]
    async fn invalidate_file_cache_of_phony_output_after_inputs() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let build = Build::new(
            vec!["foo".into()],
            vec![],
            None,
            vec!["bar".into()],
            vec![],
            None,
        );
        let context = create_build_run_context(
            &command_runner,
            &file_system,
            vec![
                build.clone(),
                explicit_build(
                    vec!["bar".into()],
                    Rule::new("touch bar".into(), None),
                    vec![],
                ),
            ],
        );
        let build = build.into();
        let mut future = pin!(run_build(context.clone(), &build));

        file_system.write_file("foo", "1");
        file_system.write_file("bar", "");

        assert!(poll!(&mut future).is_pending());

        // Let the runtime run the builds until the command of the input suspends.
        yield_now().await;

        assert_eq!(command_runner.commands(), ["touch bar"]);

        let hash = context.file_cache().content_hash(&"foo".into()).await;

        file_system.write_file("foo", "2");
        future.await.unwrap();

        assert_ne!(context.file_cache().content_hash(&"foo".into()).await, hash);
    }

    #[tokio::test]
    async fn keep_file_cache_of_other_file_after_command() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_build_run_context(&command_runner, &file_system, vec![]);

        file_system.write_file("bar", "1");

        let hash = context.file_cache().content_hash(&"bar".into()).await;

        file_system.write_file("bar", "2");
        run_build(
            context.clone(),
            &explicit_build(
                vec!["foo".into()],
                Rule::new("touch foo".into(), None),
                vec![],
            )
            .into(),
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands(), ["touch foo"]);
        assert_eq!(context.file_cache().content_hash(&"bar".into()).await, hash);
    }

    #[tokio::test]
    async fn query_output_while_building_input() {
        let file_system = FakeFileSystem::default();

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");
        file_system.write_file("baz", "");

        run(
            &create_context(&Default::default(), &Default::default(), &file_system),
            create_simple_config(
                vec![
                    explicit_build(
                        vec!["foo".into()],
                        Rule::new("cp bar foo".into(), None),
                        vec!["bar".into()],
                    ),
                    explicit_build(
                        vec!["bar".into()],
                        Rule::new("cp baz bar".into(), None),
                        vec!["baz".into()],
                    ),
                ],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(file_system.metadata_requests().first(), Some(&"foo".into()));
    }

    #[tokio::test]
    async fn query_output_once_for_up_to_date_build() {
        let file_system = FakeFileSystem::default();
        let context = create_context(&Default::default(), &Default::default(), &file_system);
        let config = create_simple_config(
            vec![
                explicit_build(
                    vec!["foo".into()],
                    Rule::new("cp bar foo".into(), None),
                    vec!["bar".into()],
                ),
                explicit_build(
                    vec!["bar".into()],
                    Rule::new("cp baz bar".into(), None),
                    vec!["baz".into()],
                ),
            ],
            &["foo"],
        );

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");
        file_system.write_file("baz", "");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();

        let count = count_metadata_requests(&file_system, "bar");

        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(count_metadata_requests(&file_system, "bar"), count + 1);
    }

    #[tokio::test]
    async fn query_implicit_output_once_for_up_to_date_build() {
        let file_system = FakeFileSystem::default();
        let context = create_context(&Default::default(), &Default::default(), &file_system);
        let config = create_simple_config(
            vec![
                explicit_build(
                    vec!["foo".into()],
                    Rule::new("cp qux foo".into(), None),
                    vec!["qux".into()],
                ),
                Build::new(
                    vec!["bar".into()],
                    vec!["qux".into()],
                    Rule::new("cp baz bar".into(), None).into(),
                    vec!["baz".into()],
                    vec![],
                    None,
                ),
            ],
            &["foo"],
        );

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");
        file_system.write_file("baz", "");
        file_system.write_file("qux", "");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();

        let count = count_metadata_requests(&file_system, "qux");

        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(count_metadata_requests(&file_system, "qux"), count + 1);
    }

    #[tokio::test]
    async fn query_output_again_on_timestamp_update_of_input() {
        let file_system = FakeFileSystem::default();
        let context = create_context(&Default::default(), &Default::default(), &file_system);
        let config = create_simple_config(
            vec![
                explicit_build(
                    vec!["foo".into()],
                    Rule::new("cp bar foo".into(), None),
                    vec!["bar".into()],
                ),
                explicit_build(
                    vec!["bar".into()],
                    Rule::new("cp baz bar".into(), None),
                    vec!["baz".into()],
                ),
            ],
            &["foo"],
        );

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");
        file_system.write_file("baz", "");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();

        file_system.write_file("baz", "");

        let count = count_metadata_requests(&file_system, "bar");

        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(count_metadata_requests(&file_system, "bar"), count + 2);
    }

    #[tokio::test]
    async fn query_output_again_after_command() {
        let file_system = FakeFileSystem::default();

        file_system.write_file("foo", "");
        file_system.write_file("bar", "");
        file_system.write_file("baz", "");

        run(
            &create_context(&Default::default(), &Default::default(), &file_system),
            create_simple_config(
                vec![
                    explicit_build(
                        vec!["foo".into()],
                        Rule::new("cp bar foo".into(), None),
                        vec!["bar".into()],
                    ),
                    explicit_build(
                        vec!["bar".into()],
                        Rule::new("cp baz bar".into(), None),
                        vec!["baz".into()],
                    ),
                ],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(count_metadata_requests(&file_system, "bar"), 2);
    }

    #[tokio::test]
    async fn query_output_again_for_up_to_date_phony_build() {
        let file_system = FakeFileSystem::default();
        let context = create_context(&Default::default(), &Default::default(), &file_system);
        let config = create_simple_config(
            vec![
                explicit_build(
                    vec!["foo.o".into()],
                    Rule::new("cc foo.c".into(), None).with_header_dependency(Some(
                        HeaderDependency::Make {
                            path: "foo.d".into(),
                        },
                    )),
                    vec!["foo.c".into()],
                ),
                Build::new(
                    vec!["foo.h".into()],
                    vec![],
                    None,
                    vec!["foo.h.in".into()],
                    vec![],
                    None,
                ),
            ],
            &["foo.o", "foo.h"],
        );

        file_system.write_file("foo.o", "");
        file_system.write_file("foo.c", "");
        file_system.write_file("foo.h", "");
        file_system.write_file("foo.h.in", "");
        file_system.write_file("foo.d", "foo.o: foo.h\n");

        run(&context, config.clone(), &[], DEFAULT_OPTIONS)
            .await
            .unwrap();

        let count = count_metadata_requests(&file_system, "foo.h");

        run(&context, config, &[], DEFAULT_OPTIONS).await.unwrap();

        assert_eq!(count_metadata_requests(&file_system, "foo.h"), count + 2);
    }

    #[tokio::test]
    async fn fail_with_output_metadata_error() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();

        file_system.write_file("bar", "");
        file_system.fail_metadata("foo");

        assert_eq!(
            run(
                &create_context(&command_runner, &Default::default(), &file_system),
                create_simple_config(
                    vec![explicit_build(
                        vec!["foo".into()],
                        Rule::new("cp bar foo".into(), None),
                        vec!["bar".into()],
                    )],
                    &["foo"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::File(FileError::new("metadata failure")))
        );
        assert_eq!(command_runner.commands(), Vec::<String>::new());
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
                    Rule::new("touch foo".into(), Some("build foo".into())),
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
                        Rule::new("exit 1".into(), None),
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
                    Rule::new("touch foo".into(), None),
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
                Rule::new("cc foo.c".into(), None).with_header_dependency(Some(
                    HeaderDependency::Make {
                        path: "foo.d".into(),
                    },
                )),
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
            Rule::new("cc foo.c".into(), None).with_header_dependency(Some(
                HeaderDependency::Gcc {
                    path: "foo.d".into(),
                },
            )),
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
            ["foo.h".into()]
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
            Rule::new("cl foo.c".into(), None).with_header_dependency(Some(
                HeaderDependency::Msvc {
                    prefix: "Note: including file: ".into(),
                },
            )),
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
            ["foo.h".into()]
        );
        assert_eq!(console.stdout(), "foo.c\n");
    }

    #[tokio::test]
    async fn intern_header_dependencies() {
        let file_system = FakeFileSystem::default();
        let context = create_context(&Default::default(), &Default::default(), &file_system);
        let build = explicit_build(
            vec!["foo.o".into()],
            Rule::new("cc foo.c".into(), None).with_header_dependency(Some(
                HeaderDependency::Gcc {
                    path: "foo.d".into(),
                },
            )),
            vec!["foo.c".into()],
        );

        file_system.write_file("foo.c", "");
        file_system.write_file("foo.h", "");
        file_system.write_file("foo.d", "foo.o: ./foo.h\n");

        run(
            &context,
            create_simple_config(vec![build.clone()], &["foo.o"]),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert!(Arc::ptr_eq(
            &context
                .database()
                .get_header_dependencies(build.id())
                .unwrap()[0],
            &context.path_pool().intern("foo.h")
        ));
    }

    #[tokio::test]
    async fn build_generated_header_dependency() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();
        let context = create_context(&command_runner, &Default::default(), &file_system);
        let build = explicit_build(
            vec!["foo.o".into()],
            Rule::new("cc foo.c".into(), None),
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
                    explicit_build(
                        vec!["foo.h".into()],
                        Rule::new("touch foo.h".into(), None),
                        vec![],
                    ),
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
            Rule::new("cc foo.c".into(), None),
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
    async fn read_header_dependencies_once() {
        let database = FakeDatabase::default();
        let file_system = FakeFileSystem::default();
        let context = Arc::new(Context::new(
            FakeCommandRunner::default(),
            Mutex::new(FakeConsole::default()).into(),
            database.clone(),
            file_system.clone(),
            Default::default(),
        ));
        let build = explicit_build(
            vec!["foo.o".into()],
            Rule::new("cc foo.c".into(), None),
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
            create_simple_config(vec![build.clone()], &["foo.o"]),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(database.header_dependency_requests(), [build.id()]);
    }

    #[tokio::test]
    async fn detect_circular_header_dependency() {
        let context = create_context(
            &Default::default(),
            &Default::default(),
            &Default::default(),
        );
        let build = explicit_build(
            vec!["foo".into()],
            Rule::new("touch foo".into(), None),
            vec![],
        );

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
                        Rule::new("touch foo".into(), None).into(),
                        vec![],
                        vec!["foo.dd".into()],
                        Some("foo.dd".into()),
                    ),
                    explicit_build(
                        vec!["foo.dd".into()],
                        Rule::new("touch foo.dd".into(), None),
                        vec![],
                    ),
                    explicit_build(
                        vec!["bar".into()],
                        Rule::new("touch bar".into(), None),
                        vec![],
                    ),
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
                        Rule::new("touch foo".into(), None).into(),
                        vec![],
                        vec!["foo.dd".into()],
                        Some("foo.dd".into()),
                    ),
                    explicit_build(
                        vec!["foo.dd".into()],
                        Rule::new("touch foo.dd".into(), None),
                        vec![],
                    ),
                    explicit_build(
                        vec!["bar".into()],
                        Rule::new("touch bar".into(), None),
                        vec![],
                    ),
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
    async fn build_dynamic_inputs_of_builds_sharing_dynamic_module() {
        let command_runner = FakeCommandRunner::default();
        let file_system = FakeFileSystem::default();

        file_system.write_file(
            "foo.dd",
            "ninja_dyndep_version = 1\nbuild foo: dyndep | baz\nbuild bar: dyndep | qux\n",
        );
        file_system.write_file("foo", "");
        file_system.write_file("baz", "");
        file_system.write_file("qux", "");

        run(
            &create_context(&command_runner, &Default::default(), &file_system),
            create_simple_config(
                vec![
                    Build::new(
                        vec!["foo".into()],
                        vec![],
                        Rule::new("touch foo".into(), None).into(),
                        vec![],
                        vec!["foo.dd".into()],
                        Some("foo.dd".into()),
                    ),
                    Build::new(
                        vec!["bar".into()],
                        vec![],
                        Rule::new("touch bar".into(), None).into(),
                        vec!["foo".into()],
                        vec!["foo.dd".into()],
                        Some("foo.dd".into()),
                    ),
                    explicit_build(
                        vec!["baz".into()],
                        Rule::new("touch baz".into(), None),
                        vec![],
                    ),
                    explicit_build(
                        vec!["qux".into()],
                        Rule::new("touch qux".into(), None),
                        vec![],
                    ),
                ],
                &["bar"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(
            command_runner.commands(),
            ["touch baz", "touch foo", "touch qux", "touch bar"]
        );
    }

    #[tokio::test]
    async fn read_dynamic_module_once() {
        let file_system = FakeFileSystem::default();

        file_system.write_file(
            "foo.dd",
            "ninja_dyndep_version = 1\nbuild foo: dyndep\nbuild bar: dyndep\n",
        );

        run(
            &create_context(&Default::default(), &Default::default(), &file_system),
            create_simple_config(
                ["foo", "bar"]
                    .into_iter()
                    .map(|output| {
                        Build::new(
                            vec![output.into()],
                            vec![],
                            Rule::new(format!("touch {output}"), None).into(),
                            vec![],
                            vec!["foo.dd".into()],
                            Some("foo.dd".into()),
                        )
                    })
                    .collect(),
                &["foo", "bar"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(file_system.read_requests(), [Path::new("foo.dd")]);
    }

    #[tokio::test]
    async fn intern_dynamic_input() {
        let file_system = FakeFileSystem::default();
        let config = create_simple_config(
            vec![Build::new(
                vec!["foo".into()],
                vec![],
                Rule::new("touch foo".into(), None).into(),
                vec![],
                vec!["foo.dd".into()],
                Some("foo.dd".into()),
            )],
            &[],
        );
        let context = RunContext::new(
            create_context(&Default::default(), &Default::default(), &file_system),
            config.clone(),
            BuildGraph::new(config.outputs()),
            Default::default(),
            DEFAULT_OPTIONS,
        );

        file_system.write_file(
            "foo.dd",
            "ninja_dyndep_version = 1\nbuild foo: dyndep | bar\n",
        );

        assert!(Arc::ptr_eq(
            &load_dynamic_config(&context, "foo.dd")
                .await
                .unwrap()
                .outputs()["foo"]
                .inputs()[0],
            &context.build().path_pool().intern("bar")
        ));
    }

    #[tokio::test]
    async fn fail_with_missing_dynamic_dependency() {
        let file_system = FakeFileSystem::default();
        let build = Build::new(
            vec!["foo".into()],
            vec![],
            Rule::new("touch foo".into(), None).into(),
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
                        Rule::new("touch foo".into(), None).into(),
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
                        Rule::new("touch foo".into(), None).into(),
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

    #[tokio::test]
    async fn run_builds_concurrently() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_simple_config(
                vec![
                    pool_build("foo", None),
                    pool_build("bar", None),
                    pool_build("baz", None),
                ],
                &["foo", "bar", "baz"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.max_concurrency(), 3);
    }

    #[tokio::test]
    async fn limit_concurrency_of_builds_in_pool() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_pool_config(
                vec![
                    pool_build("foo", limited_pool("qux")),
                    pool_build("bar", limited_pool("qux")),
                    pool_build("baz", limited_pool("qux")),
                ],
                &["foo", "bar", "baz"],
                &[("qux", 1)],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands().len(), 3);
        assert_eq!(command_runner.max_concurrency(), 1);
    }

    #[tokio::test]
    async fn limit_concurrency_of_builds_in_pool_with_depth_of_two() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_pool_config(
                vec![
                    pool_build("foo", limited_pool("quux")),
                    pool_build("bar", limited_pool("quux")),
                    pool_build("baz", limited_pool("quux")),
                    pool_build("qux", limited_pool("quux")),
                ],
                &["foo", "bar", "baz", "qux"],
                &[("quux", 2)],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands().len(), 4);
        assert_eq!(command_runner.max_concurrency(), 2);
    }

    #[tokio::test]
    async fn run_builds_in_different_pools_concurrently() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_pool_config(
                vec![
                    pool_build("foo", limited_pool("baz")),
                    pool_build("bar", limited_pool("qux")),
                ],
                &["foo", "bar"],
                &[("baz", 1), ("qux", 1)],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.max_concurrency(), 2);
    }

    #[tokio::test]
    async fn run_build_in_pool_of_huge_depth() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_pool_config(
                vec![pool_build("foo", limited_pool("bar"))],
                &["foo"],
                &[("bar", usize::MAX)],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands(), ["touch foo"]);
    }

    #[tokio::test]
    async fn run_build_in_console_pool() {
        let command_runner = FakeCommandRunner::new(
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
        );
        let console = FakeConsole::default();

        run(
            &create_context(&command_runner, &console, &Default::default()),
            create_simple_config(
                vec![explicit_build(
                    vec!["foo".into()],
                    Rule::new("touch foo".into(), Some("build foo".into()))
                        .with_pool(Some(Pool::Console)),
                    vec![],
                )],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.commands(), Vec::<String>::new());
        assert_eq!(command_runner.console_commands(), ["touch foo"]);
        assert_eq!(console.stdout(), "");
        assert_eq!(console.stderr(), "build foo\n");
    }

    #[tokio::test]
    async fn limit_concurrency_of_builds_in_console_pool() {
        let command_runner = FakeCommandRunner::default();

        run(
            &create_context(&command_runner, &Default::default(), &Default::default()),
            create_simple_config(
                vec![
                    pool_build("foo", Some(Pool::Console)),
                    pool_build("bar", Some(Pool::Console)),
                    pool_build("baz", Some(Pool::Console)),
                ],
                &["foo", "bar", "baz"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(command_runner.console_commands().len(), 3);
        assert_eq!(command_runner.max_concurrency(), 1);
    }

    #[tokio::test]
    async fn write_debug_log_of_failed_build_in_console_pool() {
        let command_runner =
            FakeCommandRunner::new([("exit 1".into(), failed_output())].into_iter().collect());
        let console = FakeConsole::default();

        assert_eq!(
            run(
                &create_context(&command_runner, &console, &Default::default()),
                create_simple_config(
                    vec![explicit_build(
                        vec!["foo".into()],
                        Rule::new("exit 1".into(), None).with_pool(Some(Pool::Console)),
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
        assert_eq!(command_runner.console_commands(), ["exit 1"]);
        assert_eq!(
            console.stderr(),
            "turtle: command: exit 1\nturtle: exit status: 1\n"
        );
    }

    #[tokio::test]
    async fn flush_console_after_build() {
        let command_runner = FakeCommandRunner::default();
        let console = FakeConsole::default();

        run(
            &create_context(&command_runner, &console, &Default::default()),
            create_simple_config(
                vec![explicit_build(
                    vec!["foo".into()],
                    Rule::new("touch foo".into(), Some("build foo".into())),
                    vec![],
                )],
                &["foo"],
            ),
            &[],
            DEFAULT_OPTIONS,
        )
        .await
        .unwrap();

        assert_eq!(console.flushed_stderr(), "build foo\n");
    }

    #[tokio::test]
    async fn write_output_of_failed_build_with_build_in_console_pool() {
        let command_runner = FakeCommandRunner::new(
            [(
                "false".into(),
                Output {
                    stderr: b"baz\n".into(),
                    ..failed_output()
                },
            )]
            .into_iter()
            .collect(),
        );
        let console = FakeConsole::default();

        assert_eq!(
            run(
                &create_context(&command_runner, &console, &Default::default()),
                create_simple_config(
                    vec![
                        explicit_build(
                            vec!["foo".into()],
                            Rule::new("touch foo".into(), None).with_pool(Some(Pool::Console)),
                            vec![],
                        ),
                        explicit_build(vec!["bar".into()], Rule::new("false".into(), None), vec![]),
                    ],
                    &["foo", "bar"],
                ),
                &[],
                DEFAULT_OPTIONS,
            )
            .await,
            Err(BuildError::Build)
        );
        assert_eq!(command_runner.console_commands(), ["touch foo"]);
        assert_eq!(command_runner.commands(), ["false"]);
        assert_eq!(console.stderr(), "baz\n");
        assert_eq!(console.flushed_stderr(), "baz\n");
    }

    #[tokio::test]
    async fn lock_console_during_command_in_console_pool() {
        let command_runner = FakeCommandRunner::default();
        let context = create_run_context(&command_runner, &Default::default(), &[]);
        let rule = Rule::new("foo".into(), None).with_pool(Some(Pool::Console));
        let mut future = pin!(run_rule(&context, &rule));

        assert!(poll!(&mut future).is_pending());
        assert_eq!(command_runner.console_commands(), ["foo"]);
        assert!(context.build().console().try_lock().is_err());

        future.await.unwrap();

        assert!(context.build().console().try_lock().is_ok());
    }

    #[tokio::test]
    async fn flush_console_before_command_in_console_pool() {
        let command_runner = FakeCommandRunner::default();
        let console = FakeConsole::default();
        let context = create_run_context(&command_runner, &console, &[]);
        let rule = Rule::new("foo".into(), Some("bar".into())).with_pool(Some(Pool::Console));
        let mut future = pin!(run_rule(&context, &rule));

        assert!(poll!(&mut future).is_pending());
        assert_eq!(command_runner.console_commands(), ["foo"]);
        assert_eq!(console.flushed_stderr(), "bar\n");

        future.await.unwrap();
    }

    #[tokio::test]
    async fn wait_for_console_before_command_in_console_pool() {
        let command_runner = FakeCommandRunner::default();
        let context = create_run_context(&command_runner, &Default::default(), &[]);
        let rule = Rule::new("foo".into(), None).with_pool(Some(Pool::Console));
        let console = context.build().console().lock().await;
        let mut future = pin!(run_rule(&context, &rule));

        for _ in 0..POLL_COUNT {
            assert!(poll!(&mut future).is_pending());
        }

        assert_eq!(command_runner.console_commands(), Vec::<String>::new());

        drop(console);
        future.await.unwrap();

        assert_eq!(command_runner.console_commands(), ["foo"]);
    }

    #[tokio::test]
    async fn run_command_concurrently_with_command_in_console_pool() {
        let command_runner = FakeCommandRunner::default();
        let context = create_run_context(&command_runner, &Default::default(), &[]);
        let foo = Rule::new("foo".into(), None).with_pool(Some(Pool::Console));
        let bar = Rule::new("bar".into(), None);
        let mut foo_future = pin!(run_rule(&context, &foo));
        let mut bar_future = pin!(run_rule(&context, &bar));

        assert!(poll!(&mut foo_future).is_pending());
        assert!(poll!(&mut bar_future).is_pending());
        assert_eq!(command_runner.console_commands(), ["foo"]);
        assert_eq!(command_runner.commands(), ["bar"]);

        foo_future.await.unwrap();
        bar_future.await.unwrap();
    }

    #[tokio::test]
    async fn queue_outputs_of_other_builds_during_command_in_console_pool() {
        let command_runner = FakeCommandRunner::new(
            [(
                "bar".into(),
                Output {
                    status: ExitStatus::default(),
                    stdout: b"baz\n".into(),
                    stderr: vec![],
                },
            )]
            .into_iter()
            .collect(),
        );
        let console = FakeConsole::default();
        let context = create_run_context(&command_runner, &console, &[]);
        let foo = Rule::new("foo".into(), Some("foo".into())).with_pool(Some(Pool::Console));
        let bar = Rule::new("bar".into(), Some("bar".into()));
        let mut foo_future = pin!(run_rule(&context, &foo));

        assert!(poll!(&mut foo_future).is_pending());
        assert_eq!(command_runner.console_commands(), ["foo"]);

        run_rule(&context, &bar).await.unwrap();

        assert_eq!(command_runner.commands(), ["bar"]);
        assert_eq!(console.stdout(), "");
        assert_eq!(console.stderr(), "foo\n");

        foo_future.await.unwrap();

        assert_eq!(console.stdout(), "baz\n");
        assert_eq!(console.stderr(), "foo\nbar\n");
    }

    #[tokio::test]
    async fn write_failure_of_other_build_after_command_in_console_pool() {
        let command_runner = FakeCommandRunner::new(
            [(
                "bar".into(),
                Output {
                    stderr: b"baz\n".into(),
                    ..failed_output()
                },
            )]
            .into_iter()
            .collect(),
        );
        let console = FakeConsole::default();
        let context = create_run_context(&command_runner, &console, &[]);
        let foo = Rule::new("foo".into(), Some("foo".into())).with_pool(Some(Pool::Console));
        let bar = Rule::new("bar".into(), Some("bar".into()));
        let mut foo_future = pin!(run_rule(&context, &foo));

        assert!(poll!(&mut foo_future).is_pending());
        assert_eq!(run_rule(&context, &bar).await, Err(BuildError::Build));
        assert_eq!(command_runner.commands(), ["bar"]);
        assert_eq!(console.stderr(), "foo\n");

        foo_future.await.unwrap();

        assert_eq!(console.stderr(), "foo\nbar\nbaz\n");
    }

    #[tokio::test]
    async fn write_queued_outputs_on_next_lock() {
        let command_runner = FakeCommandRunner::default();
        let console = FakeConsole::default();
        let context = create_run_context(&command_runner, &console, &[]);
        let rule = Rule::new("foo".into(), Some("foo".into()));
        let lock = context.build().console().lock().await;

        run_rule(&context, &rule).await.unwrap();

        assert_eq!(command_runner.commands(), ["foo"]);
        assert_eq!(console.stderr(), "");

        drop(lock);
        context.console().lock().await.unwrap();

        assert_eq!(console.stderr(), "foo\n");
    }

    #[tokio::test]
    async fn wait_for_pool_before_writing_description() {
        let command_runner = FakeCommandRunner::default();
        let console = FakeConsole::default();
        let context = create_run_context(&command_runner, &console, &[("bar", 1)]);
        let rule = Rule::new("foo".into(), Some("foo".into())).with_pool(limited_pool("bar"));
        let permit = context.pool(rule.pool()).await.unwrap();
        let mut future = pin!(run_rule(&context, &rule));

        for _ in 0..POLL_COUNT {
            assert!(poll!(&mut future).is_pending());
        }

        assert_eq!(command_runner.commands(), Vec::<String>::new());
        assert_eq!(console.stderr(), "");

        drop(permit);
        future.await.unwrap();

        assert_eq!(command_runner.commands(), ["foo"]);
        assert_eq!(console.stderr(), "foo\n");
    }

    #[tokio::test]
    async fn fail_to_write_description() {
        let command_runner = FakeCommandRunner::default();
        let context = create_run_context(&command_runner, &FakeConsole::failing(), &[]);

        assert!(matches!(
            run_rule(&context, &Rule::new("foo".into(), Some("foo".into()))).await,
            Err(BuildError::Console(_))
        ));
        assert_eq!(command_runner.commands(), Vec::<String>::new());
    }

    #[test]
    fn classify_phony_and_file_inputs() {
        let build = explicit_build(
            vec!["foo".into()],
            Rule::new("".into(), None),
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
                    explicit_build(vec!["baz".into()], Rule::new("".into(), None), vec![]),
                ],
                &[],
            ),
            BuildGraph::new(&Default::default()),
            Default::default(),
            DEFAULT_OPTIONS,
        );

        assert_eq!(
            classify_inputs(
                &context,
                &build,
                &["quux".into(), "baz".into()],
                &[&"qux".into(), &"corge".into()],
            ),
            (
                vec![&"bar".into()],
                vec![
                    &"baz".into(),
                    &"qux".into(),
                    &"quux".into(),
                    &"corge".into()
                ]
            )
        );
    }
}

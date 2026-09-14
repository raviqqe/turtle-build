mod context;
mod hash;
mod header_dependency;
mod log;
mod options;

use self::{
    context::Context as RunContext,
    hash::{calculate_content_hash, calculate_timestamp_hash},
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
use core::{future::Future, pin::Pin};
use futures::future::{FutureExt, Shared, try_join_all};
use itertools::Itertools;
pub use options::RunOptions;
use std::{path::Path, process::Output};
use tokio::{spawn, time::Instant, try_join};

type BuildFuture = Shared<Pin<Box<dyn Future<Output = Result<(), BuildError>> + Send>>>;

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

    if outputs.is_empty() {
        for output in context.config().default_outputs() {
            trigger_build(
                context.clone(),
                context
                    .config()
                    .outputs()
                    .get(output.as_ref())
                    .ok_or_else(|| BuildError::DefaultOutputNotFound(output.clone()))?,
            )
            .await?;
        }
    } else {
        for output in outputs {
            trigger_build(
                context.clone(),
                context
                    .config()
                    .outputs()
                    .get(output.as_str())
                    .ok_or_else(|| BuildError::OutputNotFound(output.clone()))?,
            )
            .await?;
        }
    }

    // Do not inline this to avoid borrowing a lock of builds.
    let futures = context
        .build_futures()
        .iter()
        .map(|r#ref| r#ref.value().clone())
        .collect::<Vec<_>>();

    let result = try_join_all(futures).await;

    context.application().database().flush().await?;

    result.map(|_| ())
}

#[async_recursion]
async fn trigger_build(context: Arc<RunContext>, build: &Arc<Build>) -> Result<(), BuildError> {
    context
        .build_futures()
        .entry(build.id())
        .or_insert_with(|| spawn_build(context.clone(), build.clone()).boxed().shared());

    Ok(())
}

async fn spawn_build(context: Arc<RunContext>, build: Arc<Build>) -> Result<(), BuildError> {
    spawn(async move {
        try_join_all(
            build
                .inputs()
                .iter()
                .chain(build.order_only_inputs())
                .map(|input| build_input(context.clone(), input)),
        )
        .await?;

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

        try_join_all(
            dynamic_inputs
                .iter()
                .map(|input| build_input(context.clone(), input)),
        )
        .await?;

        let header_dependencies = build_header_dependencies(
            &context,
            &if build.rule().is_some() {
                context
                    .application()
                    .database()
                    .get_header_dependencies(build.id())?
            } else {
                vec![]
            },
        )
        .await?;

        let outputs_exist = try_join_all(
            build
                .outputs()
                .iter()
                .chain(build.implicit_outputs())
                .map(|path| check_file_existence(&context, path)),
        )
        .await
        .is_ok();
        let (phony_inputs, file_inputs) =
            classify_inputs(&context, &build, dynamic_inputs, &header_dependencies);
        let mut timestamp_hash =
            calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        if outputs_exist
            && Some(timestamp_hash)
                == context
                    .application()
                    .database()
                    .get_hash(HashType::Timestamp, build.id())?
        {
            return Ok(());
        }

        let mut content_hash =
            calculate_content_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        if outputs_exist
            && Some(content_hash)
                == context
                    .application()
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

            let output = run_rule(&context, rule).await?;
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

                timestamp_hash =
                    calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs).await?;
                content_hash =
                    calculate_content_hash(&context, &build, &file_inputs, &phony_inputs).await?;
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

async fn build_input(context: Arc<RunContext>, input: &str) -> Result<(), BuildError> {
    if let Some(build) = context.config().outputs().get(input) {
        trigger_build(context.clone(), build).await?;

        // Do not inline this to avoid holding a lock of build futures across an await point.
        let future = context.build_futures().get(&build.id()).unwrap().clone();

        future.await
    } else {
        check_file_existence(&context, input).await
    }
}

async fn build_header_dependencies(
    context: &Arc<RunContext>,
    inputs: &[String],
) -> Result<Vec<String>, BuildError> {
    let mut futures = vec![];

    for input in inputs {
        if let Some(build) = context.config().outputs().get(input.as_str()) {
            trigger_build(context.clone(), build).await?;

            futures.push(context.build_futures().get(&build.id()).unwrap().clone());
        }
    }

    try_join_all(futures).await?;

    filter_existing_header_dependencies(context, inputs).await
}

async fn filter_existing_header_dependencies(
    context: &RunContext,
    dependencies: &[String],
) -> Result<Vec<String>, BuildError> {
    let mut existing_dependencies = vec![];

    for dependency in dependencies {
        if context
            .application()
            .file_system()
            .exists(dependency.as_ref())
            .await?
        {
            existing_dependencies.push(dependency.clone());
        }
    }

    Ok(existing_dependencies)
}

async fn check_file_existence(context: &RunContext, path: &str) -> Result<(), BuildError> {
    if !context
        .application()
        .file_system()
        .exists(path.as_ref())
        .await?
    {
        return Err(BuildError::FileNotFound(
            context
                .application()
                .database()
                .get_source(path)?
                .unwrap_or_else(|| path.into()),
        ));
    }

    Ok(())
}

async fn prepare_directory(context: &RunContext, path: impl AsRef<Path>) -> Result<(), BuildError> {
    if let Some(directory) = path.as_ref().parent() {
        context
            .application()
            .file_system()
            .create_directory(directory)
            .await?;
    }

    Ok(())
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
}

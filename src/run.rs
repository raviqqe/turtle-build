mod context;
mod hash;
mod header_dependency;
mod log;
mod options;

use self::{
    context::Context as RunContext,
    header_dependency::{exclude_show_includes, read_header_dependencies},
};
use crate::{
    build_graph::{BuildGraph, BuildGraphError},
    compile::compile_dynamic,
    context::Context,
    debug,
    error::ApplicationError,
    hash_type::HashType,
    ir::{Build, Configuration, Rule},
    parse::parse_dynamic,
    profile,
};
use async_recursion::async_recursion;
use futures::future::{FutureExt, Shared, try_join_all};
use itertools::Itertools;
pub use options::Options;
use std::{future::Future, path::Path, pin::Pin, process::Output, sync::Arc};
use tokio::{spawn, time::Instant, try_join};

type BuildFuture = Shared<Pin<Box<dyn Future<Output = Result<(), ApplicationError>> + Send>>>;

pub async fn run(
    context: &Arc<Context>,
    configuration: Arc<Configuration>,
    outputs: &[String],
    options: Options,
) -> Result<(), ApplicationError> {
    let mut graph = BuildGraph::new(configuration.outputs());

    for build in configuration
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

    let context = Arc::new(RunContext::new(
        context.clone(),
        configuration,
        graph,
        options,
    ));

    context
        .build_graph()
        .lock()
        .await
        .validate()
        .map_err(|error| map_build_graph_error(&context, &error))?;

    if outputs.is_empty() {
        for output in context.configuration().default_outputs() {
            trigger_build(
                context.clone(),
                context
                    .configuration()
                    .outputs()
                    .get(output.as_ref())
                    .ok_or_else(|| ApplicationError::DefaultOutputNotFound(output.clone()))?,
            )
            .await?;
        }
    } else {
        for output in outputs {
            trigger_build(
                context.clone(),
                context
                    .configuration()
                    .outputs()
                    .get(output.as_str())
                    .ok_or_else(|| ApplicationError::OutputNotFound(output.clone()))?,
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
async fn trigger_build(
    context: Arc<RunContext>,
    build: &Arc<Build>,
) -> Result<(), ApplicationError> {
    context
        .build_futures()
        .entry(build.id())
        .or_insert_with(|| spawn_build(context.clone(), build.clone()).boxed().shared());

    Ok(())
}

async fn spawn_build(context: Arc<RunContext>, build: Arc<Build>) -> Result<(), ApplicationError> {
    spawn(async move {
        let mut futures = vec![];

        for input in build.inputs().iter().chain(build.order_only_inputs()) {
            futures.push(build_input(context.clone(), input).await?);
        }

        // TODO Merge these with dynamic ones?
        try_join_all(futures).await?;

        // TODO Consider caching dynamic modules.
        let dynamic_configuration = if let Some(dynamic_module) = build.dynamic_module() {
            let mut source = String::new();
            context
                .application()
                .file_system()
                .read_file_to_string(dynamic_module.as_ref().as_ref(), &mut source)
                .await?;
            let configuration = compile_dynamic(&parse_dynamic(&source)?)?;

            context
                .build_graph()
                .lock()
                .await
                .validate_dynamic(&configuration)
                .map_err(|error| map_build_graph_error(&context, &error))?;

            Some(configuration)
        } else {
            None
        };

        let dynamic_inputs = if let Some(configuration) = &dynamic_configuration {
            build
                .outputs()
                .iter()
                .find_map(|output| configuration.outputs().get(output.as_ref()))
                .map(|build| build.inputs())
                .ok_or_else(|| ApplicationError::DynamicDependencyNotFound(build.clone()))?
        } else {
            &[]
        };

        let mut futures = vec![];

        for input in dynamic_inputs {
            futures.push(build_input(context.clone(), input).await?);
        }

        try_join_all(futures).await?;

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
            hash::calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs).await?;

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
            hash::calculate_content_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        if outputs_exist
            && Some(content_hash)
                == context
                    .application()
                    .database()
                    .get_hash(HashType::Content, build.id())?
        {
            return Ok(());
        }

        if let Some(rule) = build.rule() {
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

                if let Some(source) = context.configuration().source_map().get(output) {
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
                    hash::calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs)
                        .await?;
                content_hash =
                    hash::calculate_content_hash(&context, &build, &file_inputs, &phony_inputs)
                        .await?;
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

// TODO Wait for the build input?
async fn build_input(
    context: Arc<RunContext>,
    input: &str,
) -> Result<BuildFuture, ApplicationError> {
    Ok(
        if let Some(build) = context.configuration().outputs().get(input) {
            trigger_build(context.clone(), build).await?;

            context.build_futures().get(&build.id()).unwrap().clone()
        } else {
            let input = input.to_owned();

            async move { check_file_existence(&context, &input).await }
                .boxed()
                .shared()
        },
    )
}

// Unlike `build_input`, a header dependency that no longer exists is dropped
// instead of failing the build. Like ninja, this only makes the build out of
// date, since its inputs no longer match the stored hashes. A header
// dependency that is a build output is built like any other input so that
// generated headers exist before the inputs of this build are hashed.
async fn build_header_dependencies(
    context: &Arc<RunContext>,
    inputs: &[String],
) -> Result<Vec<String>, ApplicationError> {
    let mut futures = vec![];
    let mut kept = vec![];

    for input in inputs {
        if let Some(build) = context.configuration().outputs().get(input.as_str()) {
            trigger_build(context.clone(), build).await?;

            futures.push(context.build_futures().get(&build.id()).unwrap().clone());
            kept.push(input.clone());
        } else if context
            .application()
            .file_system()
            .exists(input.as_ref())
            .await?
        {
            kept.push(input.clone());
        }
    }

    try_join_all(futures).await?;

    Ok(kept)
}

async fn filter_existing_header_dependencies(
    context: &RunContext,
    dependencies: &[String],
) -> Result<Vec<String>, ApplicationError> {
    let mut existing_dependencies = vec![];

    for dependency in dependencies {
        if context
            .configuration()
            .outputs()
            .get(dependency.as_str())
            .is_none_or(|build| build.rule().is_some())
            && context
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

// TODO Use `FileSystem::exists`?
async fn check_file_existence(context: &RunContext, path: &str) -> Result<(), ApplicationError> {
    if context
        .application()
        .file_system()
        .metadata(path.as_ref())
        .await
        .is_err()
    {
        return Err(ApplicationError::FileNotFound(
            context
                .application()
                .database()
                .get_source(path)?
                .unwrap_or_else(|| path.into()),
        ));
    }

    Ok(())
}

async fn prepare_directory(
    context: &RunContext,
    path: impl AsRef<Path>,
) -> Result<(), ApplicationError> {
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
    build
        .inputs()
        .iter()
        .chain(dynamic_inputs)
        .map(AsRef::as_ref)
        .chain(header_dependencies.iter().map(String::as_str))
        .unique()
        .partition(|&input| {
            context
                .configuration()
                .outputs()
                .get(input)
                .map_or_default(|build| build.rule().is_none())
        })
}

async fn run_rule(context: &RunContext, rule: &Rule) -> Result<Output, ApplicationError> {
    let ((output, duration), mut console) = try_join!(
        async {
            let start_time = Instant::now();
            let output = context
                .application()
                .command_runner()
                .run(rule.command())
                .await?;

            Ok::<_, ApplicationError>((output, Instant::now() - start_time))
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

        return Err(ApplicationError::Build);
    }

    Ok(output)
}

fn map_build_graph_error(context: &RunContext, error: &BuildGraphError) -> ApplicationError {
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
                .collect::<Result<Vec<_>, ApplicationError>>()
            {
                Ok(outputs) => {
                    BuildGraphError::CircularDependency(outputs.into_iter().dedup().collect())
                        .into()
                }
                Err(error) => error,
            }
        }
    }
}

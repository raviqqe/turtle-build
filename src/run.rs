mod context;
mod hash;
mod log;
mod options;

use self::context::Context as RunContext;
use crate::{
    build_graph::BuildGraph,
    build_hash::BuildHash,
    compile::compile_dynamic,
    context::Context,
    debug,
    error::ApplicationError,
    ir::{Build, Configuration, Rule},
    parse::parse_dynamic,
    profile,
};
use async_recursion::async_recursion;
use futures::future::{try_join_all, FutureExt, Shared};
pub use options::Options;
use std::{future::Future, path::Path, pin::Pin, sync::Arc};
use tokio::{spawn, time::Instant, try_join};

type RawBuildFuture = Pin<Box<dyn Future<Output = Result<(), ApplicationError>> + Send>>;
type BuildFuture = Shared<RawBuildFuture>;

pub async fn run(
    context: &Arc<Context>,
    configuration: Arc<Configuration>,
    options: Options,
) -> Result<(), ApplicationError> {
    let graph = BuildGraph::new(configuration.outputs());

    graph.validate()?;

    let context = Arc::new(RunContext::new(
        context.clone(),
        configuration,
        graph,
        options,
    ));

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
        .or_insert_with(|| {
            let future: RawBuildFuture = Box::pin(spawn_build(context.clone(), build.clone()));

            future.shared()
        });

    Ok(())
}

async fn spawn_build(context: Arc<RunContext>, build: Arc<Build>) -> Result<(), ApplicationError> {
    spawn(async move {
        let mut futures = vec![];

        for input in build.inputs().iter().chain(build.order_only_inputs()) {
            futures.push(build_input(context.clone(), input).await?);
        }

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
                .validate_dynamic(&configuration)?;

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

        let outputs_exist = try_join_all(
            build
                .outputs()
                .iter()
                .chain(build.implicit_outputs())
                .map(|path| check_file_existence(&context, path)),
        )
        .await
        .is_ok();
        let old_hash = context.application().database().get_hash(build.id())?;
        let (file_inputs, phony_inputs) = build
            .inputs()
            .iter()
            .chain(dynamic_inputs)
            .map(|string| string.as_ref())
            .partition::<Vec<_>, _>(|&input| {
                if let Some(build) = context.configuration().outputs().get(input) {
                    build.rule().is_some()
                } else {
                    true
                }
            });
        let timestamp_hash =
            hash::calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        if outputs_exist && Some(timestamp_hash) == old_hash.map(|hash| hash.timestamp()) {
            return Ok(());
        }

        let content_hash =
            hash::calculate_content_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        if outputs_exist && Some(content_hash) == old_hash.map(|hash| hash.content()) {
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

            run_rule(&context, rule).await?;

            for output in build.outputs() {
                context.application().database().set_output(output)?;

                if let Some(source) = context.configuration().source_map().get(output) {
                    context
                        .application()
                        .database()
                        .set_source(output, source)?;
                }
            }
        }

        context
            .application()
            .database()
            .set_hash(build.id(), BuildHash::new(timestamp_hash, content_hash))?;

        Ok(())
    })
    .await?
}

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
            let future: RawBuildFuture =
                Box::pin(async move { check_file_existence(&context, &input).await });
            future.shared()
        },
    )
}

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

async fn run_rule(context: &RunContext, rule: &Rule) -> Result<(), ApplicationError> {
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

    profile!(context, console, "duration: {}ms", duration.as_millis());

    console.write_stdout(&output.stdout).await?;
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

    Ok(())
}

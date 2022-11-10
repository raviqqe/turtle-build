mod context;
mod hash;
mod log;
mod options;

use self::context::Context as RunContext;
use crate::{
    build_hash::BuildHash,
    compile::compile_dynamic,
    context::Context,
    debug,
    error::ApplicationError,
    ir::{Build, Configuration, Rule},
    parse::parse_dynamic,
    profile,
    validation::BuildGraph,
};
use async_recursion::async_recursion;
use futures::future::{try_join_all, FutureExt, Shared};
pub use options::Options;
use std::{future::Future, path::Path, pin::Pin, sync::Arc};
use tokio::{process::Command, spawn, sync::Semaphore, time::Instant, try_join};

type RawBuildFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), ApplicationError<'a>>> + Send + 'a>>;
type BuildFuture<'a> = Shared<RawBuildFuture<'a>>;

pub async fn run(
    context: &Arc<Context>,
    configuration: Arc<Configuration<'static>>,
    options: Options,
) -> Result<(), ApplicationError<'static>> {
    let graph = BuildGraph::new(configuration.outputs());

    graph.validate()?;

    let context = Arc::new(RunContext::new(
        context.clone(),
        configuration,
        graph,
        Semaphore::new(options.job_limit.unwrap_or_else(num_cpus::get)),
        options,
    ));

    for output in context.configuration().default_outputs() {
        trigger_build(
            context.clone(),
            context
                .configuration()
                .outputs()
                .get(output)
                .ok_or_else(|| ApplicationError::DefaultOutputNotFound(output.into()))?,
        )
        .await?;
    }

    // Do not inline this to avoid borrowing a lock of builds.
    let futures = context
        .build_futures()
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();

    // Start running build futures actually.
    if let Err(error) = try_join_all(futures).await {
        // Flush explicitly here as flush on drop doesn't work in general
        // because of possible dependency cycles of build jobs.
        context.application().database().flush().await?;

        return Err(error);
    }

    Ok(())
}

#[async_recursion]
async fn trigger_build(
    context: Arc<RunContext<'static>>,
    build: &Arc<Build<'static>>,
) -> Result<(), ApplicationError<'static>> {
    // Exclusive lock for atomic addition of a build job.
    let mut builds = context.build_futures().write().await;

    if builds.contains_key(&build.id()) {
        return Ok(());
    }

    let future: RawBuildFuture = Box::pin(spawn_build(context.clone(), build.clone()));

    builds.insert(build.id(), future.shared());

    Ok(())
}

async fn spawn_build(
    context: Arc<RunContext<'static>>,
    build: Arc<Build<'static>>,
) -> Result<(), ApplicationError<'static>> {
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
                .read_file_to_string(dynamic_module.as_ref(), &mut source)
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
                .find_map(|output| configuration.outputs().get(*output))
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
        let old_hash = context.application().database().get(build.id())?;
        let (file_inputs, phony_inputs) = build
            .inputs()
            .iter()
            .copied()
            .chain(dynamic_inputs.iter().map(|input| input.as_str()))
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
                    .map(|path| prepare_directory(&context, path)),
            )
            .await?;

            run_rule(&context, rule).await?;
        }

        context
            .application()
            .database()
            .set(build.id(), BuildHash::new(timestamp_hash, content_hash))?;

        Ok(())
    })
    .await?
}

async fn build_input(
    context: Arc<RunContext<'static>>,
    input: &str,
) -> Result<BuildFuture<'static>, ApplicationError<'static>> {
    Ok(
        if let Some(build) = context.configuration().outputs().get(input) {
            trigger_build(context.clone(), build).await?;

            context.build_futures().read().await[&build.id()].clone()
        } else {
            let input = input.to_owned();
            let future: RawBuildFuture<'static> =
                Box::pin(async move { check_file_existence(&context, &input).await });
            future.shared()
        },
    )
}

async fn check_file_existence(
    context: &RunContext<'static>,
    path: impl AsRef<Path>,
) -> Result<(), ApplicationError<'static>> {
    context
        .application()
        .file_system()
        .modified_time(path.as_ref())
        .await?;

    Ok(())
}

async fn prepare_directory(
    context: &RunContext<'_>,
    path: impl AsRef<Path>,
) -> Result<(), ApplicationError<'static>> {
    if let Some(directory) = path.as_ref().parent() {
        context
            .application()
            .file_system()
            .create_directory(directory)
            .await?;
    }

    Ok(())
}

async fn run_rule<'a>(
    context: &RunContext<'a>,
    rule: &Rule,
) -> Result<(), ApplicationError<'static>> {
    // Acquire a job semaphore first to guarantee a lock order between a job
    // semaphore and console.
    let permit = context.job_semaphore().acquire().await?;

    let ((output, duration), mut console) = try_join!(
        async {
            let start_time = Instant::now();
            let output = if cfg!(target_os = "windows") {
                let components = rule.command().split_whitespace().collect::<Vec<_>>();
                Command::new(components[0])
                    .args(&components[1..])
                    .output()
                    .await?
            } else {
                Command::new("sh")
                    .arg("-ec")
                    .arg(rule.command())
                    .output()
                    .await?
            };
            let duration = Instant::now() - start_time;

            drop(permit);

            Ok::<_, ApplicationError>((output, duration))
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

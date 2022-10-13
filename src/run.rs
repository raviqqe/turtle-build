mod build_database;
mod build_hash;
mod context;
mod hash;
mod options;

use self::{build_database::BuildDatabase, build_hash::BuildHash, context::Context};
use crate::{
    compile::compile_dynamic,
    console::Console,
    debug,
    error::InfrastructureError,
    ir::{Build, Configuration, Rule},
    parse::parse_dynamic,
    utilities::read_file,
    validation::BuildGraph,
    writeln,
};
use async_recursion::async_recursion;
use futures::future::{try_join_all, FutureExt, Shared};
pub use options::Options;
use std::{future::Future, path::Path, pin::Pin, sync::Arc};
use tokio::{
    fs::{create_dir_all, metadata},
    io::AsyncWriteExt,
    process::Command,
    spawn,
    sync::{Mutex, Semaphore},
    time::Instant,
    try_join,
};

type RawBuildFuture = Pin<Box<dyn Future<Output = Result<(), InfrastructureError>> + Send>>;
type BuildFuture = Shared<RawBuildFuture>;

pub async fn run(
    configuration: Arc<Configuration>,
    console: &Arc<Mutex<Console>>,
    build_directory: &Path,
    options: Options,
) -> Result<(), InfrastructureError> {
    let graph = BuildGraph::new(configuration.outputs());

    graph.validate()?;

    let context = Arc::new(Context::new(
        configuration,
        graph,
        BuildDatabase::new(build_directory)?,
        Semaphore::new(options.job_limit.unwrap_or_else(num_cpus::get)),
        console.clone(),
        options,
    ));

    for output in context.configuration().default_outputs() {
        trigger_build(
            &context,
            context
                .configuration()
                .outputs()
                .get(output)
                .ok_or_else(|| InfrastructureError::DefaultOutputNotFound(output.into()))?,
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
        context.database().flush().await?;

        return Err(error);
    }

    Ok(())
}

#[async_recursion]
async fn trigger_build(
    context: &Arc<Context>,
    build: &Arc<Build>,
) -> Result<(), InfrastructureError> {
    // Exclusive lock for atomic addition of a build job.
    let mut builds = context.build_futures().write().await;

    if builds.contains_key(build.id()) {
        return Ok(());
    }

    let future: RawBuildFuture = Box::pin(spawn_build(context.clone(), build.clone()));

    builds.insert(build.id().into(), future.shared());

    Ok(())
}

async fn spawn_build(context: Arc<Context>, build: Arc<Build>) -> Result<(), InfrastructureError> {
    spawn(async move {
        let mut futures = vec![];

        for input in build.inputs().iter().chain(build.order_only_inputs()) {
            if let Some(future) = build_input(&context, input).await? {
                futures.push(future);
            }
        }

        try_join_all(futures).await?;

        // TODO Consider caching dynamic modules.
        let dynamic_configuration = if let Some(dynamic_module) = build.dynamic_module() {
            let configuration =
                compile_dynamic(&parse_dynamic(&read_file(&dynamic_module).await?)?)?;

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
                .find_map(|output| configuration.outputs().get(output.as_str()))
                .map(|build| build.inputs())
                .ok_or_else(|| InfrastructureError::DynamicDependencyNotFound(build.clone()))?
        } else {
            &[]
        };

        let mut futures = vec![];

        for input in dynamic_inputs {
            if let Some(future) = build_input(&context, input).await? {
                futures.push(future);
            }
        }

        try_join_all(futures).await?;

        let outputs_exist = try_join_all(
            build
                .outputs()
                .iter()
                .chain(build.implicit_outputs())
                .map(check_file_existence),
        )
        .await
        .is_ok();
        let old_hash = context.database().get(build.id())?;
        let (file_inputs, phony_inputs) = build
            .inputs()
            .iter()
            .chain(dynamic_inputs)
            .map(|input| input.as_str())
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
                    .map(prepare_directory),
            )
            .await?;

            run_rule(&context, rule).await?;
        }

        context
            .database()
            .set(build.id(), BuildHash::new(timestamp_hash, content_hash))?;

        Ok(())
    })
    .await?
}

async fn build_input(
    context: &Arc<Context>,
    input: &str,
) -> Result<Option<BuildFuture>, InfrastructureError> {
    Ok(
        if let Some(build) = context.configuration().outputs().get(input) {
            trigger_build(context, build).await?;

            Some(context.build_futures().read().await[build.id()].clone())
        } else {
            let input = input.to_owned();
            let future: RawBuildFuture =
                Box::pin(async move { check_file_existence(&input).await });
            Some(future.shared())
        },
    )
}

async fn check_file_existence(path: impl AsRef<Path>) -> Result<(), InfrastructureError> {
    let path = path.as_ref();

    metadata(path)
        .await
        .map_err(|error| InfrastructureError::with_path(error, path))?;

    Ok(())
}

async fn prepare_directory(path: impl AsRef<Path>) -> Result<(), InfrastructureError> {
    if let Some(directory) = path.as_ref().parent() {
        create_dir_all(directory).await?;
    }

    Ok(())
}

async fn run_rule(context: &Context, rule: &Rule) -> Result<(), InfrastructureError> {
    // Acquire a job semaphore first to guarantee a lock order between a job
    // semaphore and console.
    let permit = context.job_semaphore().acquire().await?;

    let ((output, duration), mut console) = try_join!(
        async {
            let start_time = Instant::now();
            let output = if cfg!(target_os = "windows") {
                let components = rule.command().split_whitespace().collect::<Vec<_>>();
                Command::new(&components[0])
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

            Ok::<_, InfrastructureError>((output, duration))
        },
        async {
            let mut console = context.console().lock().await;

            if let Some(description) = rule.description() {
                writeln!(console.stderr(), "{}", description);
            }

            debug!(
                context.options().debug,
                console.stderr(),
                "command: {}",
                rule.command()
            );

            Ok(console)
        }
    )?;

    debug!(
        context.options().profile,
        console.stderr(),
        "duration: {}ms",
        duration.as_millis()
    );

    console.stdout().write_all(&output.stdout).await?;
    console.stderr().write_all(&output.stderr).await?;

    if !output.status.success() {
        debug!(
            context.options().debug,
            console.stderr(),
            "exit status: {}",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "-".into())
        );

        return Err(InfrastructureError::Build);
    }

    Ok(())
}

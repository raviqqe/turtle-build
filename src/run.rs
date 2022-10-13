mod build_database;
mod build_hash;
mod context;
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
use futures::future::{join_all, try_join_all, FutureExt, Shared};
pub use options::Options;
use std::{
    collections::hash_map::DefaultHasher,
    future::{ready, Future},
    hash::{Hash, Hasher},
    path::Path,
    pin::Pin,
    sync::Arc,
    time::SystemTime,
};
use tokio::{
    fs::{create_dir_all, metadata, File},
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    spawn,
    sync::{Mutex, Semaphore},
    time::Instant,
    try_join,
};

type RawBuildFuture = Pin<Box<dyn Future<Output = Result<(), InfrastructureError>> + Send>>;
type BuildFuture = Shared<RawBuildFuture>;

const BUFFER_CAPACITY: usize = 2 << 10;

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
            futures.push(build_input(context.clone(), input.to_owned()).await?);
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
            futures.push(build_input(context.clone(), input.to_owned()).await?);
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
                context
                    .configuration()
                    .outputs()
                    .get(input)
                    .map(|build| build.rule().is_some())
                    .unwrap_or_default()
            });
        let timestamp_hash =
            hash_build_with_timestamp(&context, &build, &file_inputs, &phony_inputs).await?;

        if outputs_exist && Some(timestamp_hash) == old_hash.map(|hash| hash.timestamp()) {
            return Ok(());
        }

        let content_hash = hash_build_with_content(&build, &file_inputs).await?;

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
    context: Arc<Context>,
    input: String,
) -> Result<BuildFuture, InfrastructureError> {
    Ok(
        if let Some(build) = context.configuration().outputs().get(&input) {
            if build.rule().is_none() {
                let future: RawBuildFuture = Box::pin(ready(Ok(())));
                future.shared()
            } else {
                trigger_build(&context, build).await?;

                context.build_futures().read().await[build.id()].clone()
            }
        } else {
            let future: RawBuildFuture =
                Box::pin(async move { check_file_existence(&input).await });
            future.shared()
        },
    )
}

async fn hash_build_with_timestamp(
    context: &Context,
    build: &Build,
    file_inputs: &[&str],
    phony_inputs: &[&str],
) -> Result<u64, InfrastructureError> {
    let mut hasher = DefaultHasher::new();

    hash_command(build, &mut hasher);

    join_all(file_inputs.iter().map(read_timestamp))
        .await
        .into_iter()
        .collect::<Result<Vec<SystemTime>, _>>()?
        .hash(&mut hasher);

    for &input in phony_inputs {
        context
            .database()
            .get(
                context
                    .configuration()
                    .outputs()
                    .get(input)
                    .expect("phony input build")
                    .id(),
            )?
            .expect("phony input timestamp hash")
            .timestamp()
            .hash(&mut hasher);
    }

    Ok(hasher.finish())
}

async fn hash_build_with_content(
    build: &Build,
    file_inputs: &[&str],
) -> Result<u64, InfrastructureError> {
    let mut hasher = DefaultHasher::new();

    hash_command(build, &mut hasher);

    let mut buffer = Vec::with_capacity(BUFFER_CAPACITY);

    for input in file_inputs {
        File::open(input).await?.read_to_end(&mut buffer).await?;
        buffer.hash(&mut hasher);
        buffer.clear();
    }

    Ok(hasher.finish())
}

fn hash_command(build: &Build, hasher: &mut impl Hasher) {
    build.rule().map(Rule::command).hash(hasher);
}

async fn read_timestamp(path: impl AsRef<Path>) -> Result<SystemTime, InfrastructureError> {
    let path = path.as_ref();

    metadata(path)
        .await
        .map_err(|error| InfrastructureError::with_path(error, path))?
        .modified()
        .map_err(|error| InfrastructureError::with_path(error, path))
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

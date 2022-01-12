mod build_database;
mod context;

use self::{build_database::BuildDatabase, context::Context};
use crate::{
    compile::compile_dynamic,
    error::InfrastructureError,
    ir::{Build, Configuration, Rule},
    parse::parse_dynamic,
    utilities::read_file,
};
use async_recursion::async_recursion;
use futures::future::{join_all, FutureExt, Shared};
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    future::{ready, Future},
    hash::{Hash, Hasher},
    path::Path,
    pin::Pin,
    sync::Arc,
    time::SystemTime,
};
use tokio::{
    fs::metadata,
    io::{stderr, AsyncWriteExt},
    process::Command,
    spawn,
    sync::{RwLock, Semaphore},
};

type RawBuildFuture = Pin<Box<dyn Future<Output = Result<(), InfrastructureError>> + Send>>;
type BuildFuture = Shared<RawBuildFuture>;

pub async fn run(
    configuration: &Configuration,
    build_directory: &Path,
    job_limit: Option<usize>,
) -> Result<(), InfrastructureError> {
    let context = Context::new(
        BuildDatabase::new(build_directory)?,
        Semaphore::new(job_limit.unwrap_or_else(num_cpus::get)),
    )
    .into();
    let builds = Arc::new(RwLock::new(HashMap::new()));

    // Create futures for all builds required by default outputs sequentially.
    for output in configuration.default_outputs() {
        create_build_future(
            &context,
            configuration,
            &builds,
            output,
            configuration
                .outputs()
                .get(output)
                .ok_or_else(|| InfrastructureError::DefaultOutputNotFound(output.into()))?,
        )
        .await?;
    }

    // Start running build futures actually.
    // TODO Consider await only builds of default outputs.
    select_builds(builds.read().await.values().cloned()).await?;

    Ok(())
}

#[async_recursion]
async fn create_build_future(
    context: &Arc<Context>,
    configuration: &Configuration,
    builds: &Arc<RwLock<HashMap<String, BuildFuture>>>,
    output: &str,
    build: &Arc<Build>,
) -> Result<(), InfrastructureError> {
    if builds.read().await.contains_key(build.id()) {
        return Ok(());
    }

    let mut inputs = vec![];

    for input in build.inputs().iter().chain(build.order_only_inputs()) {
        inputs.push(if let Some(build) = configuration.outputs().get(input) {
            create_build_future(context, configuration, builds, input, build).await?;

            builds.read().await[build.id()].clone()
        } else {
            let input = input.to_string();
            let raw: RawBuildFuture = Box::pin(async move { run_leaf_input(&input).await });
            raw.shared()
        });
    }

    let future = {
        let environment = (context.clone(), output.to_string(), build.clone());
        let handle = spawn(async move {
            let (context, output, build) = environment;

            select_builds(inputs).await?;

            // TODO Consider caching dynamic modules.
            let dynamic_configuration = if let Some(dynamic_module) = build.dynamic_module() {
                Some(compile_dynamic(&parse_dynamic(
                    &read_file(&dynamic_module).await?,
                )?)?)
            } else {
                None
            };

            let hash = hash_build(
                &build,
                dynamic_configuration
                    .as_ref()
                    .map(|configuration| configuration.outputs()[&output].inputs())
                    .unwrap_or_default(),
            )
            .await?;

            if hash != context.database().get(build.id())? {
                if let Some(rule) = build.rule() {
                    let permit = context.job_semaphore().acquire().await?;
                    run_command(rule.command()).await?;
                    drop(permit);
                }

                context.database().set(build.id(), hash)?;
            }

            Ok(())
        });
        let raw: RawBuildFuture = Box::pin(async move { handle.await? });
        raw.shared()
    };

    builds.write().await.insert(build.id().into(), future);

    Ok(())
}

async fn hash_build(build: &Build, dynamic_inputs: &[String]) -> Result<u64, InfrastructureError> {
    let mut hasher = DefaultHasher::new();

    build.rule().map(Rule::command).hash(&mut hasher);
    join_all(
        build
            .inputs()
            .iter()
            .chain(dynamic_inputs)
            .map(get_timestamp),
    )
    .await
    .into_iter()
    .collect::<Result<Vec<SystemTime>, _>>()?
    .hash(&mut hasher);

    Ok(hasher.finish())
}

async fn get_timestamp(path: impl AsRef<Path>) -> Result<SystemTime, InfrastructureError> {
    let path = path.as_ref();

    Ok(metadata(path)
        .await
        .map_err(|error| InfrastructureError::with_path(error, path))?
        .modified()
        .map_err(|error| InfrastructureError::with_path(error, path))?)
}

async fn select_builds(
    builds: impl IntoIterator<Item = BuildFuture>,
) -> Result<(), InfrastructureError> {
    let future: Pin<Box<dyn Future<Output = _> + Send>> = Box::pin(ready(Ok(())));

    for result in join_all(builds.into_iter().chain([future.shared()])).await {
        result?;
    }

    Ok(())
}

async fn run_leaf_input(output: &str) -> Result<(), InfrastructureError> {
    metadata(output)
        .await
        .map_err(|error| InfrastructureError::with_path(error, output))?;

    Ok(())
}

async fn run_command(command: &str) -> Result<(), InfrastructureError> {
    let output = Command::new("sh")
        .arg("-e")
        .arg("-c")
        .arg(command)
        .output()
        .await?;

    stderr().write_all(&output.stdout).await?;

    if output.status.success() {
        Ok(())
    } else {
        stderr().write_all(&output.stderr).await?;

        Err(InfrastructureError::CommandExit(
            command.into(),
            output.status.code(),
        ))
    }
}

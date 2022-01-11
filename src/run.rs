mod build_database;
mod context;

use self::{build_database::BuildDatabase, context::Context};
use crate::{
    error::InfrastructureError,
    ir::{Build, Configuration, Rule},
};
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
};

type RawBuildFuture = Pin<Box<dyn Future<Output = Result<(), InfrastructureError>> + Send>>;
type BuildFuture = Shared<RawBuildFuture>;

pub async fn run(
    configuration: &Configuration,
    build_directory: &Path,
) -> Result<(), InfrastructureError> {
    let context = Context::new(4).into();
    let database = BuildDatabase::new(build_directory)?;
    let mut builds = HashMap::new();

    select_builds(
        configuration
            .default_outputs()
            .iter()
            .map(|output| {
                Ok(run_build(
                    &context,
                    &database,
                    configuration,
                    &mut builds,
                    configuration
                        .outputs()
                        .get(output)
                        .ok_or_else(|| InfrastructureError::DefaultOutputNotFound(output.into()))?,
                ))
            })
            .collect::<Result<Vec<_>, InfrastructureError>>()?,
    )
    .await?;

    Ok(())
}

fn run_build(
    context: &Arc<Context>,
    database: &BuildDatabase,
    configuration: &Configuration,
    builds: &mut HashMap<String, BuildFuture>,
    build: &Arc<Build>,
) -> BuildFuture {
    if let Some(future) = builds.get(build.id()) {
        return future.clone();
    }

    let inputs = Arc::new(
        build
            .inputs()
            .iter()
            .chain(build.order_only_inputs())
            .map(|input| {
                if let Some(build) = configuration.outputs().get(input) {
                    run_build(&context, database, configuration, builds, build)
                } else {
                    let input = input.to_string();
                    let raw: RawBuildFuture = Box::pin(async move { run_leaf_input(&input).await });
                    raw.shared()
                }
            })
            .collect::<Vec<_>>(),
    );

    let future = {
        let environment = (context.clone(), database.clone(), build.clone());
        let handle = spawn(async move {
            let (context, database, build) = environment;

            select_builds(inputs.iter().cloned().collect::<Vec<_>>()).await?;

            let hash = hash_build(&build).await?;

            if hash != database.get(build.id())? {
                if let Some(rule) = build.rule() {
                    let permit = context.job_semaphore().acquire().await?;
                    run_command(rule.command()).await?;
                    drop(permit);
                }
            }

            database.set(build.id(), hash)?;

            Ok(())
        });
        let raw: RawBuildFuture = Box::pin(async move { handle.await? });
        raw.shared()
    };

    builds.insert(build.id().into(), future.clone());

    future
}

async fn hash_build(build: &Build) -> Result<u64, InfrastructureError> {
    let mut hasher = DefaultHasher::new();

    build.rule().map(Rule::command).hash(&mut hasher);
    join_all(build.inputs().iter().map(get_timestamp))
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

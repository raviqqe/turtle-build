mod build_database;
mod error;

use self::build_database::BuildDatabase;
use crate::ir::{Build, Configuration};
use error::RunError;
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
    io,
    io::{stderr, AsyncWriteExt},
    process::Command,
    spawn,
};

type RawBuildFuture = Pin<Box<dyn Future<Output = Result<(), RunError>> + Send>>;
type BuildFuture = Shared<RawBuildFuture>;

pub async fn run(configuration: &Configuration, build_directory: &Path) -> Result<(), RunError> {
    let database = BuildDatabase::new(build_directory)?;
    let mut builds = HashMap::new();

    select_builds(
        configuration
            .default_outputs()
            .iter()
            .map(|output| {
                Ok(run_build(
                    &database,
                    configuration,
                    &mut builds,
                    configuration
                        .outputs()
                        .get(output)
                        .ok_or_else(|| RunError::DefaultOutputNotFound(output.into()))?,
                ))
            })
            .collect::<Result<Vec<_>, RunError>>()?,
    )
    .await?;

    Ok(())
}

fn run_build(
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
            .map(|input| {
                if let Some(build) = configuration.outputs().get(input) {
                    run_build(database, configuration, builds, build)
                } else {
                    let input = input.to_string();
                    let raw: RawBuildFuture = Box::pin(async move { run_leaf_input(&input).await });
                    raw.shared()
                }
            })
            .collect::<Vec<_>>(),
    );

    let future = {
        let cloned_database = database.clone();
        let cloned_build = build.clone();
        let handle = spawn(async move {
            select_builds(inputs.iter().cloned().collect::<Vec<_>>()).await?;

            if should_build(&cloned_database, &cloned_build).await? {
                run_command(cloned_build.command()).await?;
            }

            Ok(())
        });
        let raw: RawBuildFuture = Box::pin(async move { handle.await? });
        raw.shared()
    };

    builds.insert(build.id().into(), future.clone());

    future
}

async fn should_build(database: &BuildDatabase, build: &Build) -> Result<bool, RunError> {
    let hash = {
        let mut hasher = DefaultHasher::new();

        build.command().hash(&mut hasher);
        join_all(build.inputs().iter().map(get_timestamp))
            .await
            .into_iter()
            .collect::<Result<Vec<SystemTime>, _>>()?
            .hash(&mut hasher);

        hasher.finish()
    };

    let old = database.get(build.id())?;

    database.set(build.id(), hash)?;

    Ok(hash != old)
}

async fn get_timestamp(path: impl AsRef<Path>) -> Result<SystemTime, io::Error> {
    Ok(metadata(path).await?.modified()?)
}

async fn select_builds(builds: impl IntoIterator<Item = BuildFuture>) -> Result<(), RunError> {
    let future: Pin<Box<dyn Future<Output = _> + Send>> = Box::pin(ready(Ok(())));

    for result in join_all(builds.into_iter().chain([future.shared()])).await {
        result?;
    }

    Ok(())
}

async fn run_leaf_input(output: &str) -> Result<(), RunError> {
    metadata(output).await?;

    Ok(())
}

async fn run_command(command: &str) -> Result<(), RunError> {
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

        Err(RunError::ChildExit(output.status.code()))
    }
}

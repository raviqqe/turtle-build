mod error;
mod output_database;

use crate::ir::{Build, Configuration};
use error::RunError;
use futures::future::{join_all, FutureExt, Shared};
use std::collections::HashMap;
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::Arc;
use tokio::fs::metadata;
use tokio::io::AsyncWriteExt;
use tokio::spawn;
use tokio::{io::stderr, process::Command};

type RawBuildFuture = Pin<Box<dyn Future<Output = Result<(), RunError>> + Send>>;
type BuildFuture = Shared<RawBuildFuture>;

pub async fn run(configuration: &Configuration) -> Result<(), RunError> {
    let mut builds = HashMap::new();

    select_builds(
        configuration
            .default_outputs()
            .iter()
            .map(|output| run_build(configuration, &mut builds, &configuration.outputs()[output])),
    )
    .await?;

    Ok(())
}

fn run_build(
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
                    run_build(configuration, builds, build)
                } else {
                    let input = input.to_string();
                    let raw: RawBuildFuture = Box::pin(async move { run_leaf_input(&input).await });
                    raw.shared()
                }
            })
            .collect::<Vec<_>>(),
    );

    let future = {
        let cloned_build = build.clone();
        let handle = spawn(async move {
            select_builds(inputs.iter().cloned().collect::<Vec<_>>()).await?;
            run_command(cloned_build.command()).await
        });
        let raw: RawBuildFuture = Box::pin(async move { handle.await? });
        raw.shared()
    };

    builds.insert(build.id().into(), future.clone());

    future
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

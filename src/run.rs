mod error;

use crate::ir::{Build, Configuration};
use error::RunError;
use futures::future::{select_all, FutureExt, Shared};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::spawn;
use tokio::{io::stderr, process::Command};

type BuildFuture = Shared<Pin<Box<dyn Future<Output = Result<(), RunError>> + Send>>>;

pub async fn run(configuration: &Configuration) -> Result<(), RunError> {
    let mut futures = vec![];
    let mut builds = HashMap::new();

    for (_, build) in configuration.outputs() {
        futures.push(run_build(configuration, &mut builds, build));
    }

    select_all(futures).await.0?;

    Ok(())
}

fn run_build(
    configuration: &Configuration,
    builds: &mut HashMap<String, BuildFuture>,
    build: &Build,
) -> BuildFuture {
    if let Some(future) = builds.get(build.id()) {
        return future.clone();
    }

    let inputs = Arc::new(
        build
            .inputs()
            .iter()
            .map(|input| run_build(configuration, builds, &configuration.outputs()[input]))
            .collect::<Vec<_>>(),
    );

    let rule = build.rule().clone();
    let handle = spawn(async move {
        select_all(inputs.iter().cloned()).await.0?;
        run_command(rule.command()).await
    });
    let boxed: Pin<Box<dyn Future<Output = _> + Send>> = Box::pin(async move { handle.await? });
    let future = boxed.shared();

    builds.insert(build.id().into(), future.clone());

    future
}

async fn run_command(command: &str) -> Result<(), RunError> {
    let output = Command::new("sh")
        .arg("-e")
        .arg("-c")
        .arg(command)
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        stderr().write_all(&output.stdout).await?;
        stderr().write_all(&output.stdout).await?;

        Err(RunError::ChildExit(output.status.code()).into())
    }
}

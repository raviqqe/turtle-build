mod error;

use crate::ir::{Build, Configuration};
use error::RunError;
use futures::future::{FutureExt, Shared};
use futures::stream::FuturesUnordered;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use tokio::io::AsyncWriteExt;
use tokio::spawn;
use tokio::{io::stderr, process::Command};

type BuildFuture = Shared<Pin<Box<dyn Future<Output = Result<(), RunError>>>>>;

pub async fn run(configuration: &Configuration) -> Result<(), RunError> {
    let mut builds = HashMap::new();

    for (_, build) in configuration.outputs() {
        run_build(&mut builds, build);
    }

    Ok(())
}

fn run_build(builds: &mut HashMap<String, BuildFuture>, build: &Build) -> BuildFuture {
    if let Some(future) = builds.get(build.id()) {
        return future.clone();
    }

    let rule = build.rule().clone();
    let handle = spawn(async move { run_command(rule.command()).await });
    let boxed: Pin<Box<dyn Future<Output = _>>> = Box::pin(async move { handle.await? });
    let future = boxed.shared();

    builds.insert(build.id().into(), future.clone());

    future
}

// TODO
// fn run_input_builds(
//     configuration: &Configuration,
//     builds: &mut HashMap<String, BuildFuture>,
// ) -> FuturesUnordered<BuildFuture> {
// }

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

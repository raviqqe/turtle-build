mod error;

use crate::ir::Configuration;
use error::InfrastructureError;
use std::collections::HashMap;
use std::error::Error;
use tokio::io::AsyncWriteExt;
use tokio::spawn;
use tokio::{io::stderr, process::Command};

pub async fn run(configuration: &Configuration) -> Result<(), Box<dyn Error>> {
    let mut builds = HashMap::new();

    for (_name, build) in configuration.outputs() {
        let rule = build.rule().clone();

        if !builds.contains_key(build.id()) {
            builds.insert(
                build.id(),
                spawn(async move { run_command(rule.command()).await }),
            );
        }
    }

    Ok(())
}

async fn run_command(command: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
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

        Err(InfrastructureError::ChildExit(output.status.code()).into())
    }
}

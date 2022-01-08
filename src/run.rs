mod error;

use crate::ir::Configuration;
use error::InfrastructureError;
use std::error::Error;
use tokio::io::AsyncWriteExt;
use tokio::{io::stderr, process::Command};

pub async fn run(configuration: &Configuration) -> Result<(), Box<dyn Error>> {
    Ok(())
}

async fn run_command(command: &str) -> Result<(), Box<dyn Error>> {
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

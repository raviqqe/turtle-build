use crate::{context::Context, ir::Configuration};
use futures::future::try_join_all;
use std::error::Error;
use tokio::fs::remove_file;

pub async fn clean_dead(
    context: &Context,
    configuration: &Configuration,
) -> Result<(), Box<dyn Error>> {
    try_join_all(
        context
            .database()
            .get_outputs()?
            .iter()
            .map(|output| remove_output(context, configuration, output)),
    )
    .await?;

    Ok(())
}

async fn remove_output(
    context: &Context,
    configuration: &Configuration,
    output: &str,
) -> Result<(), Box<dyn Error>> {
    if configuration.outputs().contains_key(output) {
        return Ok(());
    } else if let Ok(metadata) = context.file_system().metadata(output.as_ref()).await {
        if metadata.is_file() {
            remove_file(output).await?;
        }
    }

    Ok(())
}

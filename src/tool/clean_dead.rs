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
            .map(|output| remove_output(configuration, &output)),
    )
    .await?;

    Ok(())
}

async fn remove_output(configuration: &Configuration, output: &str) -> Result<(), Box<dyn Error>> {
    if !configuration.outputs().contains_key(output) {
        remove_file(output).await?;
    }

    Ok(())
}

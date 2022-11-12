use crate::{
    context::Context,
    ir::{BuildId, Configuration},
};
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
            .map(|(output, build_id)| remove_output(context, configuration, output, *build_id)),
    )
    .await?;

    context.database().flush().await?;

    Ok(())
}

async fn remove_output(
    context: &Context,
    configuration: &Configuration,
    output: &str,
    build_id: BuildId,
) -> Result<(), Box<dyn Error>> {
    if configuration.outputs().contains_key(output) {
        return Ok(());
    } else if let Ok(metadata) = context.file_system().metadata(output.as_ref()).await {
        context.database().remove_hash(build_id)?;

        if metadata.is_file() {
            remove_file(output).await?;
        }
    }

    Ok(())
}

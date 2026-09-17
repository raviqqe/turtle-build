use crate::{context::Context, error::BuildError, ir::Config};
use futures::future::try_join_all;

/// Cleans dead outputs.
pub async fn clean_dead(context: &Context, config: &Config) -> Result<(), BuildError> {
    try_join_all(
        context
            .database()
            .get_outputs()?
            .iter()
            .map(|output| remove_output(context, config, output)),
    )
    .await?;

    Ok(())
}

async fn remove_output(context: &Context, config: &Config, output: &str) -> Result<(), BuildError> {
    if config.outputs().contains_key(output) {
        return Ok(());
    } else if let Ok(Some(metadata)) = context.file_system().metadata(output.as_ref()).await
        && metadata.is_file()
    {
        context.file_system().remove_file(output.as_ref()).await?;
    }

    Ok(())
}

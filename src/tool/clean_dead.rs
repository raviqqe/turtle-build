use crate::{context::Context, ir::Config};
use core::error::Error;
use futures::future::try_join_all;

pub async fn clean_dead(context: &Context, config: &Config) -> Result<(), Box<dyn Error>> {
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

async fn remove_output(
    context: &Context,
    config: &Config,
    output: &str,
) -> Result<(), Box<dyn Error>> {
    if config.outputs().contains_key(output) {
        return Ok(());
    } else if let Ok(metadata) = context.file_system().metadata(output.as_ref()).await
        && metadata.is_file()
    {
        context.file_system().remove_file(output.as_ref()).await?;
    }

    Ok(())
}

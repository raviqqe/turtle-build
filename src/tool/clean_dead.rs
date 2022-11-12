use crate::{context::Context, ir::Configuration};
use std::error::Error;

pub async fn clean_dead(
    context: &Context,
    configuration: &Configuration,
) -> Result<(), Box<dyn Error>> {
    // TODO
    // try_join_all().await?;

    Ok(())
}

use crate::context::Context;
use std::error::Error;

pub async fn clean_dead(_application: &Context) -> Result<(), Box<dyn Error>> {
    Ok(())
}

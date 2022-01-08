mod ast;
mod parse;

use ast::Module;
use parse::parse;
use std::error::Error;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    read_configuration().await?;

    Ok(())
}

async fn read_configuration() -> Result<Module, Box<dyn Error>> {
    let mut source = "".into();

    File::open("build.ninja")
        .await?
        .read_to_string(&mut source)
        .await?;

    Ok(parse(&source)?)
}

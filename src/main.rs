mod ast;
mod compile;
mod ir;
mod parse;
mod run;

use ast::Module;
use compile::compile;
use parse::parse;
use run::run;
use std::error::Error;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run(&compile(&read_configuration().await?)?).await?;

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

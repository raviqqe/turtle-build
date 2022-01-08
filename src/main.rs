mod ast;
mod compile;
mod ir;
mod parse;
mod run;

use ast::Module;
use compile::compile;
use parse::parse;
use run::run;
use std::collections::HashMap;
use std::error::Error;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let root_module_path = "build.ninja";
    run(&compile(
        &read_modules(root_module_path).await?,
        root_module_path,
    )?)
    .await?;

    Ok(())
}

async fn read_modules(path: &str) -> Result<HashMap<String, Module>, Box<dyn Error>> {
    let mut paths = vec![path.to_string()];
    let mut modules = HashMap::new();

    while let Some(path) = paths.pop() {
        let module = read_module(&path).await?;

        paths.extend(
            module
                .submodules()
                .iter()
                .map(|submodule| submodule.path().to_string()),
        );

        modules.insert(path, module);
    }

    Ok(modules)
}

async fn read_module(path: &str) -> Result<Module, Box<dyn Error>> {
    let mut source = "".into();

    File::open(path).await?.read_to_string(&mut source).await?;

    Ok(parse(&source)?)
}

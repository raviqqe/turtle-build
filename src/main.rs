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
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let root_module_path = PathBuf::from("build.ninja");

    run(&compile(
        &read_modules(&root_module_path).await?,
        &root_module_path,
    )?)
    .await?;

    Ok(())
}

async fn read_modules(path: &Path) -> Result<HashMap<PathBuf, Module>, Box<dyn Error>> {
    let mut paths = vec![path.to_owned()];
    let mut modules = HashMap::new();

    while let Some(path) = paths.pop() {
        let path = path.canonicalize()?;
        let module = read_module(&path).await?;

        paths.extend(
            module
                .submodules()
                .iter()
                .map(|submodule| path.join(submodule.path())),
        );

        modules.insert(path, module);
    }

    Ok(modules)
}

async fn read_module(path: &Path) -> Result<Module, Box<dyn Error>> {
    let mut source = "".into();

    File::open(path).await?.read_to_string(&mut source).await?;

    Ok(parse(&source)?)
}

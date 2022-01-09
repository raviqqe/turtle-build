mod ast;
mod compile;
mod ir;
mod parse;
mod run;

use ast::Module;
use compile::{compile, ModuleDependencyMap};
use parse::parse;
use run::run;
use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let root_module_path = PathBuf::from("build.ninja").canonicalize()?;
    let (modules, dependencies) = read_modules(&root_module_path).await?;

    run(&compile(&modules, &dependencies, &root_module_path)?).await?;

    Ok(())
}

async fn read_modules(
    path: &Path,
) -> Result<(HashMap<PathBuf, Module>, ModuleDependencyMap), Box<dyn Error>> {
    let mut paths = vec![path.canonicalize()?];
    let mut modules = HashMap::new();
    let mut dependencies = HashMap::new();

    while let Some(path) = paths.pop() {
        let module = read_module(&path).await?;

        let submodule_paths = module
            .statements()
            .iter()
            .filter_map(|statement| statement.as_submodule())
            .map(|submodule| {
                Ok((
                    submodule.path().into(),
                    path.parent()
                        .unwrap()
                        .join(submodule.path())
                        .canonicalize()?,
                ))
            })
            .collect::<Result<HashMap<_, _>, io::Error>>()?;

        paths.extend(submodule_paths.values().cloned());

        modules.insert(path.clone(), module);
        dependencies.insert(path, submodule_paths);
    }

    Ok((modules, dependencies))
}

async fn read_module(path: &Path) -> Result<Module, Box<dyn Error>> {
    let mut source = "".into();

    File::open(path).await?.read_to_string(&mut source).await?;

    Ok(parse(&source)?)
}

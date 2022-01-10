mod arguments;
mod ast;
mod compile;
mod error;
mod ir;
mod parse;
mod run;

use arguments::Arguments;
use ast::{Module, Statement};
use clap::Parser;
use compile::{compile, ModuleDependencyMap};
use error::InfrastructureError;
use parse::parse;
use run::run;
use std::{
    collections::HashMap,
    error::Error,
    io,
    path::{Path, PathBuf},
    process::exit,
};
use tokio::{fs::File, io::AsyncReadExt};

const DEFAULT_BUILD_FILE: &str = "build.ninja";

#[tokio::main]
async fn main() {
    if let Err(error) = execute().await {
        eprintln!("{}", error);

        exit(1)
    }
}

async fn execute() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse();

    let root_module_path = {
        let path = PathBuf::from(&arguments.file.as_deref().unwrap_or(DEFAULT_BUILD_FILE));

        path.canonicalize()
            .map_err(|error| InfrastructureError::with_path(error, path))?
    };
    let (modules, dependencies) = read_modules(&root_module_path).await?;

    run(
        &compile(&modules, &dependencies, &root_module_path)?,
        root_module_path.parent().unwrap(),
    )
    .await?;

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
            .filter_map(|statement| match statement {
                Statement::Include(include) => Some(include.path()),
                Statement::Submodule(submodule) => Some(submodule.path()),
                _ => None,
            })
            .map(|submodule_path| {
                Ok((
                    submodule_path.into(),
                    path.parent().unwrap().join(submodule_path).canonicalize()?,
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

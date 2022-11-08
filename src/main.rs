mod arguments;
mod ast;
mod compile;
mod console;
mod error;
mod ir;
mod parse;
mod run;
mod utilities;
mod validation;

use arguments::Arguments;
use ast::{Module, Statement};
use clap::Parser;
use compile::{compile, ModuleDependencyMap};
use console::Console;
use error::InfrastructureError;
use futures::future::try_join_all;
use parse::parse;
use std::{
    collections::HashMap,
    env::set_current_dir,
    path::{Path, PathBuf},
    process::exit,
    sync::Arc,
    time::Duration,
};
use tokio::{io::AsyncWriteExt, sync::Mutex, time::sleep};
use utilities::{canonicalize_path, read_file};
use validation::validate_modules;

const DEFAULT_BUILD_FILE: &str = "build.ninja";

#[tokio::main]
async fn main() {
    let arguments = Arguments::parse();
    let console = Arc::new(Mutex::new(Console::new()));

    if let Err(error) = execute(&arguments, &console).await {
        if !(arguments.quiet && matches!(error, InfrastructureError::Build)) {
            console
                .lock()
                .await
                .stderr()
                .write_all(
                    format!(
                        "{}{}\n",
                        if let Some(prefix) = &arguments.log_prefix {
                            prefix
                        } else {
                            ""
                        },
                        error
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }

        // Delay for the error message to be written completely hopefully.
        sleep(Duration::from_millis(1)).await;

        exit(1)
    }
}

async fn execute(
    arguments: &Arguments,
    console: &Arc<Mutex<Console>>,
) -> Result<(), InfrastructureError> {
    if let Some(directory) = &arguments.directory {
        set_current_dir(directory)?;
    }

    let root_module_path =
        canonicalize_path(&arguments.file.as_deref().unwrap_or(DEFAULT_BUILD_FILE)).await?;
    let (modules, dependencies) = read_modules(&root_module_path).await?;

    validate_modules(&dependencies)?;

    let configuration = Arc::new(compile(&modules, &dependencies, &root_module_path)?);
    let build_directory = configuration
        .build_directory()
        .map(PathBuf::from)
        .unwrap_or_else(|| root_module_path.parent().unwrap().into());

    run::run(
        configuration.clone(),
        console,
        &build_directory,
        run::Options {
            debug: arguments.debug,
            job_limit: arguments.job_limit,
            profile: arguments.profile,
        },
    )
    .await
    .map_err(|error| error.map_outputs(configuration.source_map()))?;

    Ok(())
}

async fn read_modules(
    path: &Path,
) -> Result<(HashMap<PathBuf, Module<'static>>, ModuleDependencyMap), InfrastructureError> {
    let mut paths = vec![canonicalize_path(path).await?];
    let mut modules = HashMap::new();
    let mut dependencies = HashMap::new();

    while let Some(path) = paths.pop() {
        // HACK Leak sources.
        let source = Box::leak::<'static>(read_file(&path).await?.into_boxed_str());
        let module = parse(source)?;

        let submodule_paths = try_join_all(
            module
                .statements()
                .iter()
                .filter_map(|statement| match statement {
                    Statement::Include(include) => Some(include.path()),
                    Statement::Submodule(submodule) => Some(submodule.path()),
                    _ => None,
                })
                .map(|submodule_path| resolve_submodule_path(&path, submodule_path))
                .collect::<Vec<_>>(),
        )
        .await?
        .into_iter()
        .collect::<HashMap<_, _>>();

        paths.extend(submodule_paths.values().cloned());

        modules.insert(path.clone(), module);
        dependencies.insert(path, submodule_paths);
    }

    Ok((modules, dependencies))
}

async fn resolve_submodule_path(
    module_path: &Path,
    submodule_path: &str,
) -> Result<(String, PathBuf), InfrastructureError> {
    Ok((
        submodule_path.into(),
        canonicalize_path(module_path.parent().unwrap().join(submodule_path)).await?,
    ))
}

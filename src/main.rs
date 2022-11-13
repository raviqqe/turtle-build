mod arguments;
mod ast;
mod build_hash;
mod compile;
mod context;
mod error;
mod infrastructure;
mod ir;
mod module_dependency_map;
mod parse;
mod parse_modules;
mod run;
mod tool;
mod validation;

use arguments::{Arguments, Tool};
use clap::Parser;
use compile::compile;
use context::Context;
use error::ApplicationError;
use infrastructure::{OsCommandRunner, OsConsole, OsDatabase, OsFileSystem};
use parse_modules::parse_modules;
use std::{env::set_current_dir, process::exit, sync::Arc, time::Duration};
use tokio::time::sleep;
use validation::validate_modules;

const DEFAULT_BUILD_FILE: &str = "build.ninja";
const DATABASE_DIRECTORY: &str = ".turtle";
const OPEN_FILE_LIMIT: usize = if cfg!(target_os = "macos") { 256 } else { 1024 };
const DEFAULT_FILE_COUNT_PER_PROCESS: usize = 3; // stdin, stdout, and stderr

#[tokio::main]
async fn main() {
    let arguments = Arguments::parse();
    let job_limit = arguments.job_limit.unwrap_or_else(num_cpus::get);
    let context = Context::new(
        OsCommandRunner::new(job_limit),
        OsConsole::new(),
        OsDatabase::new(),
        OsFileSystem::new(
            OPEN_FILE_LIMIT
                .saturating_sub(DEFAULT_FILE_COUNT_PER_PROCESS * (job_limit + 1))
                .max(1),
        ),
    )
    .into();

    if let Err(error) = execute(&context, &arguments).await {
        if !arguments.quiet || !matches!(error, ApplicationError::Build) {
            context
                .console()
                .lock()
                .await
                .write_stderr(
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

async fn execute(context: &Arc<Context>, arguments: &Arguments) -> Result<(), ApplicationError> {
    if let Some(directory) = &arguments.directory {
        set_current_dir(directory)?;
    }

    let root_module_path = context
        .file_system()
        .canonicalize_path(
            arguments
                .file
                .as_deref()
                .unwrap_or(DEFAULT_BUILD_FILE)
                .as_ref(),
        )
        .await?;
    let (modules, dependencies) = parse_modules(context, &root_module_path).await?;

    validate_modules(&dependencies)?;

    let configuration = Arc::new(compile(&modules, &dependencies, &root_module_path)?);

    context.database().initialize(
        &configuration
            .build_directory()
            .map(|string| string.as_ref().as_ref())
            .unwrap_or_else(|| root_module_path.parent().unwrap())
            .join(DATABASE_DIRECTORY)
            .join(env!("CARGO_PKG_VERSION").replace('.', "_")),
    )?;

    if let Some(tool) = &arguments.tool {
        match tool {
            Tool::CleanDead => tool::clean_dead(context, &configuration).await?,
        }
    } else {
        run::run(
            context,
            configuration.clone(),
            run::Options {
                debug: arguments.debug,
                profile: arguments.profile,
            },
        )
        .await
        .map_err(|error| error.map_outputs(|output| context.database().get_source(output)))?;
    }

    Ok(())
}

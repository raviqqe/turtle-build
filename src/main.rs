#![doc = include_str!("../README.md")]

use clap::{Parser, ValueEnum};
use core::error::Error;
use futures::future::try_join_all;
#[cfg(unix)]
use rlimit::Resource;
use rlimit::increase_nofile_limit;
use std::{
    collections::HashMap,
    env::set_current_dir,
    path::{Path, PathBuf},
    process::exit,
    sync::Arc,
    time::Duration,
};
use tokio::time::sleep;
use turtle_build::{
    BuildError, Context, FjallDatabase, Module, ModuleDependencyMap, OsCommandRunner, OsConsole,
    OsFileSystem, RunOptions, Statement, clean_dead, compile, parse, run,
    validate_module_dependencies,
};

const DEFAULT_BUILD_FILE: &str = "build.ninja";
const DATABASE_DIRECTORY: &str = ".turtle";
const DEFAULT_FILE_COUNT_PER_PROCESS: usize = 3; // stdin, stdout, and stderr

#[derive(Parser)]
#[clap(about = "The Ninja build system clone written in Rust", version)]
struct Arguments {
    #[clap(help = "Specify outputs")]
    outputs: Vec<String>,
    #[clap(short, help = "Set a root build file")]
    file: Option<String>,
    #[clap(short = 'C', help = "Set a working directory")]
    directory: Option<String>,
    #[clap(short, help = "Set a job limit")]
    job_limit: Option<usize>,
    #[clap(long, help = "Set a log prefix")]
    log_prefix: Option<String>,
    #[clap(long, help = "Show no message on failure of build jobs")]
    quiet: bool,
    #[clap(long, help = "Show debug logs", env = "TURTLE_DEBUG")]
    debug: bool,
    #[clap(long, help = "Show profile timings", env = "TURTLE_PROFILE")]
    profile: bool,
    #[clap(short, help = "Use a complementary tool")]
    tool: Option<Tool>,
}

#[derive(Clone, ValueEnum)]
#[clap(rename_all = "lower")]
enum Tool {
    CleanDead,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse();
    let job_limit = arguments.job_limit.unwrap_or_else(num_cpus::get);

    increase_nofile_limit(u64::MAX)?;

    let context = Context::new(
        OsCommandRunner::new(job_limit),
        OsConsole::new(),
        FjallDatabase::new(),
        OsFileSystem::new(
            cfg_select! {
                unix => usize::try_from(Resource::NOFILE.get_soft()?)?,
                _ => usize::MAX,
            }
            .saturating_sub(DEFAULT_FILE_COUNT_PER_PROCESS * (job_limit + 1))
            .max(1),
        ),
    )
    .into();

    if let Err(error) = execute(&context, &arguments).await {
        if !arguments.quiet || !matches!(error, BuildError::Build) {
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

    Ok(())
}

async fn execute(context: &Arc<Context>, arguments: &Arguments) -> Result<(), BuildError> {
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

    validate_module_dependencies(&dependencies)?;

    let config = Arc::new(compile(&modules, &dependencies, &root_module_path)?);

    context.database().initialize(
        &config
            .build_directory()
            .map(|string| string.as_ref().as_ref())
            .unwrap_or_else(|| root_module_path.parent().unwrap())
            .join(DATABASE_DIRECTORY)
            .join(env!("CARGO_PKG_VERSION").replace('.', "_")),
    )?;

    if let Some(tool) = &arguments.tool {
        match tool {
            Tool::CleanDead => clean_dead(context, &config).await?,
        }
    } else {
        run(
            context,
            config.clone(),
            &arguments.outputs,
            RunOptions {
                debug: arguments.debug,
                profile: arguments.profile,
            },
        )
        .await?;
    }

    Ok(())
}

async fn parse_modules(
    context: &Context,
    path: &Path,
) -> Result<(HashMap<PathBuf, Module>, ModuleDependencyMap), BuildError> {
    let mut paths = vec![context.file_system().canonicalize_path(path).await?];
    let mut modules = HashMap::new();
    let mut dependencies = HashMap::new();

    while let Some(path) = paths.pop() {
        let mut source = String::new();

        context
            .file_system()
            .read_file_to_string(&path, &mut source)
            .await?;

        let module = parse(&source)?;

        let submodule_paths = try_join_all(
            module
                .statements()
                .iter()
                .filter_map(|statement| match statement {
                    Statement::Include(include) => Some(include.path()),
                    Statement::Submodule(submodule) => Some(submodule.path()),
                    _ => None,
                })
                .map(|submodule_path| resolve_submodule_path(context, &path, submodule_path))
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
    context: &Context,
    module_path: &Path,
    submodule_path: &str,
) -> Result<(String, PathBuf), BuildError> {
    Ok((
        submodule_path.into(),
        context
            .file_system()
            .canonicalize_path(&module_path.parent().unwrap().join(submodule_path))
            .await?,
    ))
}

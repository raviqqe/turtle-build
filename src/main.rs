#![doc = include_str!("../README.md")]

extern crate alloc;

use alloc::sync::Arc;
use clap::{Parser, ValueEnum};
use core::error::Error;
use core::time::Duration;
use futures::future::try_join_all;
#[cfg(unix)]
use rlimit::Resource;
use rlimit::increase_nofile_limit;
use std::{
    collections::HashMap,
    env::set_current_dir,
    path::{Path, PathBuf},
    process::exit,
};
use tokio::{sync::Mutex, time::sleep};
use turtle_build::{
    BuildError, Console, Context, FileSystem, Module, ModuleDependencyMap, OsCommandRunner,
    OsConsole, OsFileSystem, PathPool, RedbDatabase, RunOptions, Statement, clean_dead, compile,
    job_limit, parse, run,
};

const DEFAULT_BUILD_FILE: &str = "build.ninja";
const DATABASE_DIRECTORY: &str = ".turtle";
const DATABASE_EXTENSION: &str = "redb";
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
    let console = Arc::new(Mutex::new(OsConsole::new()));

    if let Err(error) = execute(&arguments, &console).await {
        if !arguments.quiet || !matches!(error, BuildError::Build) {
            console
                .lock()
                .await
                .write_stderr(
                    format!(
                        "{}{}\n",
                        arguments.log_prefix.as_deref().unwrap_or_default(),
                        error
                    )
                    .as_bytes(),
                )
                .await?;
        }

        // Delay for the error message to be written completely hopefully.
        sleep(Duration::from_millis(1)).await;

        exit(1)
    }

    Ok(())
}

async fn execute(arguments: &Arguments, console: &Arc<Mutex<OsConsole>>) -> Result<(), BuildError> {
    if let Some(directory) = &arguments.directory {
        set_current_dir(directory)?;
    }

    increase_nofile_limit(u64::MAX)?;

    let job_limit = arguments
        .job_limit
        .unwrap_or_else(|| job_limit(num_cpus::get()));
    let file_system = OsFileSystem::new(
        cfg_select! {
            unix => usize::try_from(Resource::NOFILE.get_soft()?).unwrap_or(usize::MAX),
            _ => usize::MAX,
        }
        .saturating_sub(DEFAULT_FILE_COUNT_PER_PROCESS * (job_limit + 1))
        .max(1),
    );
    let root_module_path = file_system
        .canonicalize_path(
            arguments
                .file
                .as_deref()
                .unwrap_or(DEFAULT_BUILD_FILE)
                .as_ref(),
        )
        .await?;
    let (modules, dependencies) = parse_modules(&file_system, &root_module_path).await?;

    let path_pool = Arc::new(PathPool::new());
    let config = Arc::new(compile(
        &modules,
        &dependencies,
        &root_module_path,
        &path_pool,
    )?);
    let context = Arc::new(Context::new(
        OsCommandRunner::new(job_limit),
        console.clone(),
        RedbDatabase::new(
            &config
                .build_directory()
                .map(|string| string.as_ref().as_ref())
                .unwrap_or_else(|| root_module_path.parent().unwrap())
                .join(DATABASE_DIRECTORY)
                .join(env!("CARGO_PKG_VERSION").replace('.', "_"))
                .with_extension(DATABASE_EXTENSION),
            path_pool.clone(),
        )?,
        file_system,
        path_pool,
    ));

    if let Some(tool) = &arguments.tool {
        match tool {
            Tool::CleanDead => clean_dead(&context, &config).await?,
        }
    } else {
        run(
            &context,
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
    file_system: &OsFileSystem,
    path: &Path,
) -> Result<(HashMap<PathBuf, Module>, ModuleDependencyMap), BuildError> {
    let mut paths = vec![(file_system.canonicalize_path(path).await?, vec![])];
    let mut modules = HashMap::new();
    let mut dependencies = HashMap::new();

    while let Some((path, mut ancestors)) = paths.pop() {
        if let Some(index) = ancestors.iter().position(|ancestor| ancestor == &path) {
            return Err(BuildError::CircularModuleDependency(
                ancestors[index..].to_vec(),
            ));
        } else if modules.contains_key(&path) {
            continue;
        }

        let module = parse(&file_system.read_file_to_string(&path).await?)?;

        let submodule_paths = try_join_all(
            module
                .statements()
                .iter()
                .filter_map(|statement| match statement {
                    Statement::Include(include) => Some(include.path()),
                    Statement::Submodule(submodule) => Some(submodule.path()),
                    _ => None,
                })
                .map(|path| resolve_submodule_path(file_system, path))
                .collect::<Vec<_>>(),
        )
        .await?
        .into_iter()
        .collect::<HashMap<_, _>>();

        ancestors.push(path.clone());
        paths.extend(
            submodule_paths
                .values()
                .map(|path| (path.clone(), ancestors.clone())),
        );

        modules.insert(path.clone(), module);
        dependencies.insert(path, submodule_paths);
    }

    Ok((modules, dependencies))
}

async fn resolve_submodule_path(
    file_system: &OsFileSystem,
    path: &str,
) -> Result<(String, PathBuf), BuildError> {
    // TODO Interpolate variables in paths of included and sub-ninja files like ninja.
    Ok((
        path.into(),
        file_system.canonicalize_path(path.as_ref()).await?,
    ))
}

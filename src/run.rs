mod context;
mod hash;
mod log;
mod options;

use self::context::Context as RunContext;
use crate::{
    build_graph::{BuildGraph, BuildGraphError},
    compile::compile_dynamic,
    context::Context,
    debug, depfile,
    error::ApplicationError,
    hash_type::HashType,
    ir::{Build, Configuration, DependencyStyle, Rule},
    parse::parse_dynamic,
    profile,
};
use async_recursion::async_recursion;
use futures::future::{FutureExt, Shared, try_join_all};
use itertools::Itertools;
pub use options::Options;
use std::{collections::HashSet, future::Future, path::Path, pin::Pin, process::Output, sync::Arc};
use tokio::{spawn, time::Instant, try_join};

type RawBuildFuture = Pin<Box<dyn Future<Output = Result<(), ApplicationError>> + Send>>;
type BuildFuture = Shared<RawBuildFuture>;

pub async fn run(
    context: &Arc<Context>,
    configuration: Arc<Configuration>,
    outputs: &[String],
    options: Options,
) -> Result<(), ApplicationError> {
    let graph = BuildGraph::new(configuration.outputs());
    let context = Arc::new(RunContext::new(
        context.clone(),
        configuration,
        graph,
        options,
    ));

    context
        .build_graph()
        .lock()
        .await
        .validate()
        .map_err(|error| map_build_graph_error(&context, &error))?;

    if outputs.is_empty() {
        for output in context.configuration().default_outputs() {
            trigger_build(
                context.clone(),
                context
                    .configuration()
                    .outputs()
                    .get(output.as_ref())
                    .ok_or_else(|| ApplicationError::DefaultOutputNotFound(output.clone()))?,
            )
            .await?;
        }
    } else {
        for output in outputs {
            trigger_build(
                context.clone(),
                context
                    .configuration()
                    .outputs()
                    .get(output.as_str())
                    .ok_or_else(|| ApplicationError::OutputNotFound(output.clone()))?,
            )
            .await?;
        }
    }

    // Do not inline this to avoid borrowing a lock of builds.
    let futures = context
        .build_futures()
        .iter()
        .map(|r#ref| r#ref.value().clone())
        .collect::<Vec<_>>();

    let result = try_join_all(futures).await;

    context.application().database().flush().await?;

    result.map(|_| ())
}

#[async_recursion]
async fn trigger_build(
    context: Arc<RunContext>,
    build: &Arc<Build>,
) -> Result<(), ApplicationError> {
    context
        .build_futures()
        .entry(build.id())
        .or_insert_with(|| {
            let future: RawBuildFuture = Box::pin(spawn_build(context.clone(), build.clone()));

            future.shared()
        });

    Ok(())
}

async fn spawn_build(context: Arc<RunContext>, build: Arc<Build>) -> Result<(), ApplicationError> {
    spawn(async move {
        let mut futures = vec![];

        for input in build.inputs().iter().chain(build.order_only_inputs()) {
            futures.push(build_input(context.clone(), input).await?);
        }

        try_join_all(futures).await?;

        // TODO Consider caching dynamic modules.
        let dynamic_configuration = if let Some(dynamic_module) = build.dynamic_module() {
            let mut source = String::new();
            context
                .application()
                .file_system()
                .read_file_to_string(dynamic_module.as_ref().as_ref(), &mut source)
                .await?;
            let configuration = compile_dynamic(&parse_dynamic(&source)?)?;

            context
                .build_graph()
                .lock()
                .await
                .validate_dynamic(&configuration)
                .map_err(|error| map_build_graph_error(&context, &error))?;

            Some(configuration)
        } else {
            None
        };

        let dynamic_inputs = if let Some(configuration) = &dynamic_configuration {
            build
                .outputs()
                .iter()
                .find_map(|output| configuration.outputs().get(output.as_ref()))
                .map(|build| build.inputs())
                .ok_or_else(|| ApplicationError::DynamicDependencyNotFound(build.clone()))?
        } else {
            &[]
        };

        let mut futures = vec![];

        for input in dynamic_inputs {
            futures.push(build_input(context.clone(), input).await?);
        }

        try_join_all(futures).await?;

        let discovered_dependencies = context
            .application()
            .database()
            .get_discovered_dependencies(build.id())?;
        let mut futures = vec![];

        for input in &discovered_dependencies {
            futures.push(build_input(context.clone(), input).await?);
        }

        try_join_all(futures).await?;

        let outputs_exist = try_join_all(
            build
                .outputs()
                .iter()
                .chain(build.implicit_outputs())
                .map(|path| check_file_existence(&context, path)),
        )
        .await
        .is_ok();
        let (file_inputs, phony_inputs) =
            classify_inputs(&context, &build, dynamic_inputs, &discovered_dependencies);
        let timestamp_hash =
            hash::calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        if outputs_exist
            && Some(timestamp_hash)
                == context
                    .application()
                    .database()
                    .get_hash(HashType::Timestamp, build.id())?
        {
            return Ok(());
        }

        let content_hash =
            hash::calculate_content_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        if outputs_exist
            && Some(content_hash)
                == context
                    .application()
                    .database()
                    .get_hash(HashType::Content, build.id())?
        {
            return Ok(());
        } else if let Some(rule) = build.rule() {
            try_join_all(
                build
                    .outputs()
                    .iter()
                    .chain(build.implicit_outputs())
                    .map(|path| prepare_directory(&context, path.as_ref())),
            )
            .await?;

            let discovered_dependencies = run_rule(&context, rule).await?;

            context
                .application()
                .database()
                .set_discovered_dependencies(build.id(), &discovered_dependencies)?;

            for output in build.outputs() {
                context.application().database().set_output(output)?;

                if let Some(source) = context.configuration().source_map().get(output) {
                    context
                        .application()
                        .database()
                        .set_source(output, source)?;
                }
            }
        }

        let discovered_dependencies = context
            .application()
            .database()
            .get_discovered_dependencies(build.id())?;
        let (file_inputs, phony_inputs) =
            classify_inputs(&context, &build, dynamic_inputs, &discovered_dependencies);
        let timestamp_hash =
            hash::calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs).await?;
        let content_hash =
            hash::calculate_content_hash(&context, &build, &file_inputs, &phony_inputs).await?;

        context.application().database().set_hash(
            HashType::Timestamp,
            build.id(),
            timestamp_hash,
        )?;
        context
            .application()
            .database()
            .set_hash(HashType::Content, build.id(), content_hash)?;

        Ok(())
    })
    .await?
}

async fn build_input(
    context: Arc<RunContext>,
    input: &str,
) -> Result<BuildFuture, ApplicationError> {
    Ok(
        if let Some(build) = context.configuration().outputs().get(input) {
            trigger_build(context.clone(), build).await?;

            context.build_futures().get(&build.id()).unwrap().clone()
        } else {
            let input = input.to_owned();
            let future: RawBuildFuture =
                Box::pin(async move { check_file_existence(&context, &input).await });
            future.shared()
        },
    )
}

async fn check_file_existence(context: &RunContext, path: &str) -> Result<(), ApplicationError> {
    if context
        .application()
        .file_system()
        .metadata(path.as_ref())
        .await
        .is_err()
    {
        return Err(ApplicationError::FileNotFound(
            context
                .application()
                .database()
                .get_source(path)?
                .unwrap_or_else(|| path.into()),
        ));
    }

    Ok(())
}

async fn prepare_directory(
    context: &RunContext,
    path: impl AsRef<Path>,
) -> Result<(), ApplicationError> {
    if let Some(directory) = path.as_ref().parent() {
        context
            .application()
            .file_system()
            .create_directory(directory)
            .await?;
    }

    Ok(())
}

fn classify_inputs<'a>(
    context: &'a RunContext,
    build: &'a Build,
    dynamic_inputs: &'a [Arc<str>],
    discovered_dependencies: &'a [String],
) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut seen = HashSet::new();

    build
        .inputs()
        .iter()
        .chain(dynamic_inputs)
        .map(|string| string.as_ref())
        .chain(discovered_dependencies.iter().map(String::as_str))
        .filter(|input| seen.insert(*input))
        .partition::<Vec<_>, _>(|&input| {
            if let Some(build) = context.configuration().outputs().get(input) {
                build.rule().is_some()
            } else {
                true
            }
        })
}

async fn run_rule(context: &RunContext, rule: &Rule) -> Result<Vec<String>, ApplicationError> {
    let ((output, duration), mut console) = try_join!(
        async {
            let start_time = Instant::now();
            let output = context
                .application()
                .command_runner()
                .run(rule.command())
                .await?;

            Ok::<_, ApplicationError>((output, Instant::now() - start_time))
        },
        async {
            let mut console = context.application().console().lock().await;

            if let Some(description) = rule.description() {
                console.write_stderr(description.as_bytes()).await?;
                console.write_stderr(b"\n").await?;
            }

            debug!(context, console, "command: {}", rule.command());

            Ok(console)
        }
    )?;
    let output = read_rule_output(context, rule, output).await?;

    profile!(context, console, "duration: {}ms", duration.as_millis());

    console.write_stdout(&output.stdout).await?;
    console.write_stderr(&output.stderr).await?;

    if !output.status.success() {
        debug!(
            context,
            console,
            "exit status: {}",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "-".into())
        );

        return Err(ApplicationError::Build);
    }

    Ok(output.discovered_dependencies)
}

async fn read_rule_output(
    context: &RunContext,
    rule: &Rule,
    output: Output,
) -> Result<RuleOutput, ApplicationError> {
    let (mut discovered_dependencies, stdout) =
        if rule.dependency_style() == Some(DependencyStyle::Msvc) {
            extract_show_includes(output.stdout)
        } else {
            (vec![], output.stdout)
        };

    if let Some(depfile) = rule.depfile() {
        discovered_dependencies.extend(read_depfile(context, depfile).await?);
    }

    deduplicate_strings(&mut discovered_dependencies);

    Ok(RuleOutput {
        discovered_dependencies,
        stderr: output.stderr,
        status: output.status,
        stdout,
    })
}

async fn read_depfile(context: &RunContext, path: &str) -> Result<Vec<String>, ApplicationError> {
    let mut source = String::new();

    match context
        .application()
        .file_system()
        .read_file_to_string(path.as_ref(), &mut source)
        .await
    {
        Ok(()) => depfile::parse(&source).map_err(ApplicationError::Other),
        Err(error) if error.to_string().starts_with("No such file or directory:") => Ok(vec![]),
        Err(error) => Err(error.into()),
    }
}

fn extract_show_includes(output: Vec<u8>) -> (Vec<String>, Vec<u8>) {
    const PREFIX: &[u8] = b"Note: including file: ";

    let mut filtered_output = vec![];
    let mut includes = vec![];

    for line in output.split(|&byte| byte == b'\n') {
        if let Some(include) = line.strip_prefix(PREFIX) {
            let include = include
                .iter()
                .skip_while(|&&byte| byte == b' ')
                .copied()
                .collect::<Vec<_>>();
            let include = if include.last() == Some(&b'\r') {
                &include[..include.len().saturating_sub(1)]
            } else {
                &include
            };

            includes.push(String::from_utf8_lossy(include).into_owned());
        } else {
            if !filtered_output.is_empty() {
                filtered_output.push(b'\n');
            }

            filtered_output.extend_from_slice(line);
        }
    }

    (includes, filtered_output)
}

fn deduplicate_strings(strings: &mut Vec<String>) {
    let mut seen = HashSet::new();

    strings.retain(|string| seen.insert(string.clone()));
}

struct RuleOutput {
    discovered_dependencies: Vec<String>,
    stderr: Vec<u8>,
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
}

fn map_build_graph_error(context: &RunContext, error: &BuildGraphError) -> ApplicationError {
    match error {
        BuildGraphError::CircularDependency(outputs) => {
            match outputs
                .iter()
                .map(|output| {
                    Ok::<_, ApplicationError>(
                        context
                            .application()
                            .database()
                            .get_source(output)?
                            .map(|string| string.into())
                            .unwrap_or_else(|| output.clone()),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(outputs) => {
                    BuildGraphError::CircularDependency(outputs.into_iter().dedup().collect())
                        .into()
                }
                Err(error) => error,
            }
        }
    }
}

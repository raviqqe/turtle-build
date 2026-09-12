mod context;
mod hash;
mod log;
mod options;

use self::context::Context as RunContext;
use crate::{
    build_graph::{BuildGraph, BuildGraphError},
    compile::compile_dynamic,
    context::Context,
    debug,
    error::ApplicationError,
    file::canonicalize_path,
    hash_type::HashType,
    ir::{Build, Configuration, Dependency, Rule},
    parse::{parse_depfile, parse_dynamic},
    profile,
};
use async_recursion::async_recursion;
use futures::future::{FutureExt, Shared, try_join_all};
use itertools::Itertools;
pub use options::Options;
use std::{future::Future, path::Path, pin::Pin, process::Output, sync::Arc};
use tokio::{spawn, time::Instant, try_join};

type BuildFuture = Shared<Pin<Box<dyn Future<Output = Result<(), ApplicationError>> + Send>>>;

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
        .or_insert_with(|| spawn_build(context.clone(), build.clone()).boxed().shared());

    Ok(())
}

async fn spawn_build(context: Arc<RunContext>, build: Arc<Build>) -> Result<(), ApplicationError> {
    spawn(async move {
        let mut futures = vec![];

        for input in build.inputs().iter().chain(build.order_only_inputs()) {
            futures.push(build_input(context.clone(), input).await?);
        }

        // TODO Merge these with dynamic ones?
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

        // Only known outputs have a rule that could ever discover
        // dependencies, so builds without one (e.g. phony builds) never have
        // an entry to read back.
        let stale_discovered_dependencies = if build.rule().is_some() {
            context
                .application()
                .database()
                .get_discovered_dependencies(build.id())?
        } else {
            vec![]
        };

        // A cycle through a dependency discovered on a previous run (e.g. a
        // generated header that itself depends on this build's output) is
        // invisible to the static graph, so gotta check before we
        // start awaiting futures for it, or two builds would await each
        // other forever.
        if !stale_discovered_dependencies.is_empty() {
            context
                .build_graph()
                .lock()
                .await
                .validate_discovered_dependencies(
                    &build.outputs()[0],
                    &stale_discovered_dependencies,
                )
                .map_err(|error| map_build_graph_error(&context, &error))?;
        }

        let mut discovered_dependencies =
            build_discovered_dependencies(&context, &stale_discovered_dependencies).await?;

        let outputs_exist = try_join_all(
            build
                .outputs()
                .iter()
                .chain(build.implicit_outputs())
                .map(|path| check_file_existence(&context, path)),
        )
        .await
        .is_ok();
        let (phony_inputs, file_inputs) =
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
        }

        let mut discovered_dependencies_changed = false;

        if let Some(rule) = build.rule() {
            try_join_all(
                build
                    .outputs()
                    .iter()
                    .chain(build.implicit_outputs())
                    .map(|path| prepare_directory(&context, path.as_ref())),
            )
            .await?;

            let new_discovered_dependencies = run_rule(&context, rule).await?;

            if !new_discovered_dependencies.is_empty() {
                context
                    .build_graph()
                    .lock()
                    .await
                    .validate_discovered_dependencies(
                        &build.outputs()[0],
                        &new_discovered_dependencies,
                    )
                    .map_err(|error| map_build_graph_error(&context, &error))?;
            }

            // TODO Record newly discovered dependencies without building them.
            // The command has already run, so hashing generated headers built
            // here marks this build up to date against inputs it never saw.
            let new_discovered_dependencies =
                build_discovered_dependencies(&context, &new_discovered_dependencies).await?;

            context
                .application()
                .database()
                .set_discovered_dependencies(build.id(), &new_discovered_dependencies)?;

            for output in build.outputs() {
                context.application().database().set_output(output)?;

                if let Some(source) = context.configuration().source_map().get(output) {
                    context
                        .application()
                        .database()
                        .set_source(output, source)?;
                }
            }

            discovered_dependencies_changed =
                discovered_dependencies != new_discovered_dependencies;
            discovered_dependencies = new_discovered_dependencies;
        }

        // A rule is not expected to modify its own inputs, so hashes only
        // need to be recomputed here when the discovered dependency set
        // itself changed
        let (timestamp_hash, content_hash) = if discovered_dependencies_changed {
            let (phony_inputs, file_inputs) =
                classify_inputs(&context, &build, dynamic_inputs, &discovered_dependencies);

            (
                hash::calculate_timestamp_hash(&context, &build, &file_inputs, &phony_inputs)
                    .await?,
                hash::calculate_content_hash(&context, &build, &file_inputs, &phony_inputs).await?,
            )
        } else {
            (timestamp_hash, content_hash)
        };

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

// TODO Wait for the build input?
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

            async move { check_file_existence(&context, &input).await }
                .boxed()
                .shared()
        },
    )
}

// Unlike `build_input`, a dependency discovered by a rule's own command (via
// a depfile or `/showIncludes`) can't turn to hard error just
// because it went missing.
//
// Against Ninja, it treats that as "this build is dirty," not
// as a failure. A discovered dependency that is a known build output is
// still built like any other input, since generated headers must exist
// before their consumer's inputs are hashed.
async fn build_discovered_dependencies(
    context: &Arc<RunContext>,
    inputs: &[String],
) -> Result<Vec<String>, ApplicationError> {
    let mut futures = vec![];
    let mut kept = vec![];

    for input in inputs {
        if let Some(build) = context.configuration().outputs().get(input.as_str()) {
            trigger_build(context.clone(), build).await?;

            futures.push(context.build_futures().get(&build.id()).unwrap().clone());
            kept.push(input.clone());
        } else if context
            .application()
            .file_system()
            .exists(input.as_ref())
            .await?
        {
            kept.push(input.clone());
        }
    }

    try_join_all(futures).await?;

    Ok(kept)
}

// TODO Use `FileSystem::exists`?
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
    build
        .inputs()
        .iter()
        .chain(dynamic_inputs)
        .map(AsRef::as_ref)
        .chain(discovered_dependencies.iter().map(String::as_str))
        .unique()
        .partition::<Vec<_>, _>(|&input| {
            context
                .configuration()
                .outputs()
                .get(input)
                .map_or_default(|build| build.rule().is_none())
        })
}

async fn run_rule(context: &RunContext, rule: &Rule) -> Result<Vec<String>, ApplicationError> {
    let ((mut output, duration), mut console) = try_join!(
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

    let dependencies = read_rule_output(context, rule, &mut output).await?;

    profile!(context, console, "duration: {} ms", duration.as_millis());

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

    // Ninja absorbs a gcc-style depfile into its own dependency tracking and
    // then deletes it. As turtle's database plays that role, a depfile is only
    // left on disk for a failed command. A command may also not write one at
    // all, in which case there is nothing to clean up.
    if let Some(Dependency::Gcc { path }) = rule.dependency()
        && context
            .application()
            .file_system()
            .exists(path.as_ref())
            .await?
    {
        context
            .application()
            .file_system()
            .remove_file(path.as_ref())
            .await?;
    }

    Ok(dependencies)
}

async fn read_rule_output(
    context: &RunContext,
    rule: &Rule,
    output: &mut Output,
) -> Result<Vec<String>, ApplicationError> {
    let mut discovered_dependencies = match rule.dependency() {
        None => vec![],
        Some(Dependency::Depfile { path } | Dependency::Gcc { path }) => {
            read_depfile(context, path).await?
        }
        Some(Dependency::Msvc { prefix }) => {
            let (includes, stdout) = extract_show_includes(&output.stdout, prefix.as_bytes());

            output.stdout = stdout;

            includes
        }
    };

    for dependency in &mut discovered_dependencies {
        *dependency = canonicalize_path(dependency);
    }

    Ok(discovered_dependencies)
}

async fn read_depfile(context: &RunContext, path: &str) -> Result<Vec<String>, ApplicationError> {
    if !context
        .application()
        .file_system()
        .exists(path.as_ref())
        .await?
    {
        return Ok(vec![]);
    }

    let mut source = String::new();

    context
        .application()
        .file_system()
        .read_file_to_string(path.as_ref(), &mut source)
        .await?;

    Ok(parse_depfile(&source)?)
}

fn extract_show_includes(output: &[u8], prefix: &[u8]) -> (Vec<String>, Vec<u8>) {
    let mut filtered_output = vec![];
    let mut includes = vec![];
    let mut first_line = true;

    for line in output.split(|&byte| byte == b'\n') {
        if let Some(include) = line.strip_prefix(prefix) {
            includes.push(String::from_utf8_lossy(include.trim_ascii()).into_owned());
        } else {
            if !first_line {
                filtered_output.push(b'\n');
            }

            first_line = false;

            filtered_output.extend_from_slice(line);
        }
    }

    (includes, filtered_output)
}

fn map_build_graph_error(context: &RunContext, error: &BuildGraphError) -> ApplicationError {
    match error {
        BuildGraphError::CircularDependency(outputs) => {
            match outputs
                .iter()
                .map(|output| {
                    Ok(context
                        .application()
                        .database()
                        .get_source(output)?
                        .map(|string| string.into())
                        .unwrap_or_else(|| output.clone()))
                })
                .collect::<Result<Vec<_>, ApplicationError>>()
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn extract_show_includes_with_no_output() {
        assert_eq!(
            extract_show_includes(b"", b"Note: including file: "),
            (vec![], vec![])
        );
    }

    #[test]
    fn extract_show_includes_with_leading_blank_line() {
        assert_eq!(
            extract_show_includes(b"\nAAA\n\nBBB\n", b"Note: including file: "),
            (vec![], b"\nAAA\n\nBBB\n".to_vec())
        );
    }

    #[test]
    fn extract_show_includes_with_only_includes() {
        assert_eq!(
            extract_show_includes(
                b"Note: including file: foo.h\nNote: including file: bar.h\n",
                b"Note: including file: "
            ),
            (vec!["foo.h".into(), "bar.h".into()], vec![])
        );
    }

    #[test]
    fn extract_show_includes_interleaved_with_output() {
        assert_eq!(
            extract_show_includes(
                b"AAA\nNote: including file: foo.h\nBBB\n",
                b"Note: including file: "
            ),
            (vec!["foo.h".into()], b"AAA\nBBB\n".to_vec())
        );
    }

    #[test]
    fn extract_show_includes_with_windows_line_endings() {
        assert_eq!(
            extract_show_includes(
                b"Note: including file: foo.h\r\nAAA\r\n",
                b"Note: including file: "
            ),
            (vec!["foo.h".into()], b"AAA\r\n".to_vec())
        );
    }

    #[test]
    fn extract_show_includes_with_indented_include() {
        assert_eq!(
            extract_show_includes(b"Note: including file:  foo.h\n", b"Note: including file: "),
            (vec!["foo.h".into()], vec![])
        );
    }

    #[test]
    fn extract_show_includes_with_custom_prefix() {
        assert_eq!(
            extract_show_includes(b"Hinweis: foo.h\n", b"Hinweis: "),
            (vec!["foo.h".into()], vec![])
        );
    }
}

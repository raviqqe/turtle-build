use super::context::RunContext;
use crate::{
    error::BuildError,
    hash_type::HashType,
    ir::{Build, Rule},
};
use core::hash::{Hash, Hasher};
use std::{collections::hash_map::DefaultHasher, time::SystemTime};

pub fn hash_content(content: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();

    content.hash(&mut hasher);

    hasher.finish()
}

pub fn calculate_timestamp_hash(
    context: &RunContext,
    build: &Build,
    modified_times: &[SystemTime],
    phony_inputs: &[&str],
) -> Result<u64, BuildError> {
    calculate_hash(
        context,
        build,
        HashType::Timestamp,
        modified_times,
        phony_inputs,
    )
}

pub fn calculate_content_hash(
    context: &RunContext,
    build: &Build,
    content_hashes: &[u64],
    phony_inputs: &[&str],
) -> Result<u64, BuildError> {
    calculate_hash(
        context,
        build,
        HashType::Content,
        content_hashes,
        phony_inputs,
    )
}

fn calculate_hash(
    context: &RunContext,
    build: &Build,
    r#type: HashType,
    file_hashes: &[impl Hash],
    phony_inputs: &[&str],
) -> Result<u64, BuildError> {
    if let Some(hash) = calculate_phony_hash(build, file_hashes.len(), phony_inputs) {
        return Ok(hash);
    }

    let mut hasher = DefaultHasher::new();

    hash_command(build, &mut hasher);

    for hash in file_hashes {
        hash.hash(&mut hasher);
    }

    for &input in phony_inputs {
        get_build_hash(context, r#type, input)?.hash(&mut hasher);
    }

    Ok(hasher.finish())
}

fn get_build_hash(context: &RunContext, r#type: HashType, input: &str) -> Result<u64, BuildError> {
    context
        .application()
        .database()
        .get_hash(
            r#type,
            context
                .config()
                .outputs()
                .get(input)
                .ok_or_else(|| BuildError::InputNotFound(input.into()))?
                .id(),
        )?
        .ok_or_else(|| BuildError::InputNotBuilt(input.into()))
}

fn calculate_phony_hash(
    build: &Build,
    file_input_count: usize,
    phony_inputs: &[&str],
) -> Option<u64> {
    if build.rule().is_none() && file_input_count == 0 && phony_inputs.is_empty() {
        Some(rand::random())
    } else {
        None
    }
}

fn hash_command(build: &Build, hasher: &mut impl Hasher) {
    let rule = build.rule();

    rule.map(Rule::command).hash(hasher);
    rule.and_then(Rule::header_dependency).hash(hasher);
}

#[cfg(test)]
mod tests {
    use super::{super::RunOptions, *};
    use crate::{
        build_graph::BuildGraph,
        context::Context,
        infrastructure::{FakeCommandRunner, FakeConsole, FakeDatabase, FakeFileSystem},
        ir::{Config, HeaderDependency},
    };
    use alloc::sync::Arc;
    use core::time::Duration;
    use pretty_assertions::{assert_eq, assert_ne};

    fn create_context(builds: Vec<Build>) -> RunContext {
        RunContext::new(
            Context::new(
                FakeCommandRunner::default(),
                FakeConsole::default(),
                FakeDatabase::default(),
                FakeFileSystem::default(),
            )
            .into(),
            Config::new(
                builds
                    .into_iter()
                    .map(|build| (build.outputs()[0].clone(), Arc::new(build)))
                    .collect(),
                Default::default(),
                Default::default(),
                None,
            )
            .into(),
            BuildGraph::new(&Default::default()),
            RunOptions {
                debug: false,
                profile: false,
            },
        )
    }

    #[test]
    fn keep_timestamp_hash_format() {
        let build = Build::new(
            vec!["foo.o".into()],
            vec![],
            Rule::new("cc foo.c", None).into(),
            vec!["foo.c".into(), "foo.h".into()],
            vec![],
            None,
        );
        let modified_times = [
            SystemTime::UNIX_EPOCH,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ];
        let mut hasher = DefaultHasher::new();

        Some("cc foo.c").hash(&mut hasher);
        None::<&HeaderDependency>.hash(&mut hasher);

        for time in modified_times {
            time.hash(&mut hasher);
        }

        assert_eq!(
            calculate_timestamp_hash(&create_context(vec![]), &build, &modified_times, &[]),
            Ok(hasher.finish())
        );
    }

    #[test]
    fn hash_same_content_equally() {
        assert_eq!(hash_content(b"foo"), hash_content(b"foo"));
    }

    #[test]
    fn hash_different_content_differently() {
        assert_ne!(hash_content(b"foo"), hash_content(b"bar"));
    }

    #[test]
    fn distinguish_file_hashes_in_content_hash() {
        let build = Build::new(
            vec!["foo".into()],
            vec![],
            Rule::new("cat bar baz", None).into(),
            vec!["bar".into(), "baz".into()],
            vec![],
            None,
        );
        let context = create_context(vec![]);

        assert_ne!(
            calculate_content_hash(&context, &build, &[1, 2], &[]),
            calculate_content_hash(&context, &build, &[2, 1], &[])
        );
    }

    #[test]
    fn randomize_hash_of_phony_build_without_inputs() {
        let build = Build::new(vec!["foo".into()], vec![], None, vec![], vec![], None);
        let context = create_context(vec![]);

        assert_ne!(
            calculate_timestamp_hash(&context, &build, &[], &[]),
            calculate_timestamp_hash(&context, &build, &[], &[])
        );
    }

    #[test]
    fn fail_with_phony_input_not_built() {
        let context = create_context(vec![Build::new(
            vec!["bar".into()],
            vec![],
            None,
            vec![],
            vec![],
            None,
        )]);

        assert_eq!(
            calculate_content_hash(
                &context,
                &Build::new(
                    vec!["foo".into()],
                    vec![],
                    None,
                    vec!["bar".into()],
                    vec![],
                    None,
                ),
                &[],
                &["bar"],
            ),
            Err(BuildError::InputNotBuilt("bar".into()))
        );
    }
}

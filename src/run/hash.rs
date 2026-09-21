use super::context::RunContext;
use crate::{
    error::BuildError,
    hash_type::HashType,
    ir::{Build, Rule},
};
use alloc::sync::Arc;
use core::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

pub async fn calculate_timestamp_hash(
    context: &RunContext,
    build: &Build,
    file_inputs: &[&Arc<str>],
    phony_inputs: &[&Arc<str>],
) -> Result<u64, BuildError> {
    if let Some(hash) = calculate_phony_hash(build, file_inputs, phony_inputs) {
        return Ok(hash);
    }

    let mut hasher = DefaultHasher::new();

    hash_command(build, &mut hasher);

    for &input in file_inputs {
        context
            .file_cache()
            .metadata(input)
            .await?
            .ok_or_else(|| BuildError::FileNotFound(input.as_ref().into()))?
            .modified_time()
            .hash(&mut hasher);
    }

    for &input in phony_inputs {
        get_build_hash(context, HashType::Timestamp, input)?.hash(&mut hasher);
    }

    Ok(hasher.finish())
}

pub async fn calculate_content_hash(
    context: &RunContext,
    build: &Build,
    file_inputs: &[&Arc<str>],
    phony_inputs: &[&Arc<str>],
) -> Result<u64, BuildError> {
    if let Some(hash) = calculate_phony_hash(build, file_inputs, phony_inputs) {
        return Ok(hash);
    }

    let mut hasher = DefaultHasher::new();

    hash_command(build, &mut hasher);

    for &input in file_inputs {
        context
            .file_cache()
            .content_hash(input)
            .await?
            .hash(&mut hasher);
    }

    for &input in phony_inputs {
        get_build_hash(context, HashType::Content, input)?.hash(&mut hasher);
    }

    Ok(hasher.finish())
}

fn get_build_hash(context: &RunContext, r#type: HashType, input: &str) -> Result<u64, BuildError> {
    context
        .build()
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
    file_inputs: &[&Arc<str>],
    phony_inputs: &[&Arc<str>],
) -> Option<u64> {
    if build.rule().is_none() && file_inputs.is_empty() && phony_inputs.is_empty() {
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
    use core::{
        hash::{BuildHasher, BuildHasherDefault},
        time::Duration,
    };
    use pretty_assertions::{assert_eq, assert_ne};
    use std::time::SystemTime;
    use tokio::sync::Mutex;

    fn create_context(file_system: &FakeFileSystem, builds: Vec<Build>) -> RunContext {
        RunContext::new(
            Context::new(
                FakeCommandRunner::default(),
                Mutex::new(FakeConsole::default()).into(),
                FakeDatabase::default(),
                file_system.clone(),
                Default::default(),
            )
            .into(),
            Config::new(
                builds
                    .into_iter()
                    .map(|build| (build.outputs()[0].clone(), Arc::new(build)))
                    .collect(),
                Default::default(),
                Default::default(),
                Default::default(),
                None,
            )
            .into(),
            BuildGraph::new(&Default::default()),
            Default::default(),
            RunOptions {
                debug: false,
                profile: false,
            },
        )
    }

    #[tokio::test]
    async fn keep_timestamp_hash_format() {
        let file_system = FakeFileSystem::default();
        let build = Build::new(
            vec!["foo.o".into()],
            vec![],
            Rule::new("cc foo.c".into(), None).into(),
            vec!["foo.c".into(), "foo.h".into()],
            vec![],
            None,
        );
        let modified_times = [
            SystemTime::UNIX_EPOCH,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        ];
        let mut hasher = DefaultHasher::new();

        file_system.write_file("foo.c", "");
        file_system.write_file("foo.h", "");

        Some("cc foo.c").hash(&mut hasher);
        None::<&HeaderDependency>.hash(&mut hasher);

        for time in modified_times {
            time.hash(&mut hasher);
        }

        assert_eq!(
            calculate_timestamp_hash(
                &create_context(&file_system, vec![]),
                &build,
                &[&"foo.c".into(), &"foo.h".into()],
                &[]
            )
            .await,
            Ok(hasher.finish())
        );
    }

    #[tokio::test]
    async fn cache_modified_times_in_timestamp_hash() {
        let file_system = FakeFileSystem::default();
        let build = Build::new(
            vec!["foo".into()],
            vec![],
            Rule::new("cat bar".into(), None).into(),
            vec!["bar".into()],
            vec![],
            None,
        );
        let context = create_context(&file_system, vec![]);

        file_system.write_file("bar", "");

        let hash = calculate_timestamp_hash(&context, &build, &[&"bar".into()], &[]).await;

        file_system.write_file("bar", "");

        assert_eq!(
            hash,
            calculate_timestamp_hash(&context, &build, &[&"bar".into()], &[]).await
        );
    }

    #[tokio::test]
    async fn keep_content_hash_format() {
        let file_system = FakeFileSystem::default();
        let build = Build::new(
            vec!["foo.o".into()],
            vec![],
            Rule::new("cc foo.c".into(), None).into(),
            vec!["foo.c".into(), "foo.h".into()],
            vec![],
            None,
        );
        let mut hasher = DefaultHasher::new();

        file_system.write_file("foo.c", "foo");
        file_system.write_file("foo.h", "bar");

        Some("cc foo.c").hash(&mut hasher);
        None::<&HeaderDependency>.hash(&mut hasher);

        for content in ["foo", "bar"] {
            BuildHasherDefault::<DefaultHasher>::default()
                .hash_one(content.as_bytes())
                .hash(&mut hasher);
        }

        assert_eq!(
            calculate_content_hash(
                &create_context(&file_system, vec![]),
                &build,
                &[&"foo.c".into(), &"foo.h".into()],
                &[]
            )
            .await,
            Ok(hasher.finish())
        );
    }

    #[tokio::test]
    async fn distinguish_file_hashes_in_content_hash() {
        let file_system = FakeFileSystem::default();
        let build = Build::new(
            vec!["foo".into()],
            vec![],
            Rule::new("cat bar baz".into(), None).into(),
            vec!["bar".into(), "baz".into()],
            vec![],
            None,
        );

        file_system.write_file("bar", "1");
        file_system.write_file("baz", "2");

        let hash = calculate_content_hash(
            &create_context(&file_system, vec![]),
            &build,
            &[&"bar".into(), &"baz".into()],
            &[],
        )
        .await;

        file_system.write_file("bar", "2");
        file_system.write_file("baz", "1");

        assert_ne!(
            hash,
            calculate_content_hash(
                &create_context(&file_system, vec![]),
                &build,
                &[&"bar".into(), &"baz".into()],
                &[]
            )
            .await
        );
    }

    #[tokio::test]
    async fn cache_file_hashes_in_content_hash() {
        let file_system = FakeFileSystem::default();
        let build = Build::new(
            vec!["foo".into()],
            vec![],
            Rule::new("cat bar".into(), None).into(),
            vec!["bar".into()],
            vec![],
            None,
        );
        let context = create_context(&file_system, vec![]);

        file_system.write_file("bar", "1");

        let hash = calculate_content_hash(&context, &build, &[&"bar".into()], &[]).await;

        file_system.write_file("bar", "2");

        assert_eq!(
            hash,
            calculate_content_hash(&context, &build, &[&"bar".into()], &[]).await
        );
    }

    #[tokio::test]
    async fn randomize_hash_of_phony_build_without_inputs() {
        let build = Build::new(vec!["foo".into()], vec![], None, vec![], vec![], None);
        let context = create_context(&Default::default(), vec![]);

        assert_ne!(
            calculate_timestamp_hash(&context, &build, &[], &[]).await,
            calculate_timestamp_hash(&context, &build, &[], &[]).await
        );
    }

    #[tokio::test]
    async fn fail_with_missing_file_input() {
        assert_eq!(
            calculate_timestamp_hash(
                &create_context(&Default::default(), vec![]),
                &Build::new(
                    vec!["foo".into()],
                    vec![],
                    Rule::new("cat bar".into(), None).into(),
                    vec!["bar".into()],
                    vec![],
                    None,
                ),
                &[&"bar".into()],
                &[],
            )
            .await,
            Err(BuildError::FileNotFound("bar".into()))
        );
    }

    #[tokio::test]
    async fn fail_with_phony_input_not_built() {
        let context = create_context(
            &Default::default(),
            vec![Build::new(
                vec!["bar".into()],
                vec![],
                None,
                vec![],
                vec![],
                None,
            )],
        );

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
                &[&"bar".into()],
            )
            .await,
            Err(BuildError::InputNotBuilt("bar".into()))
        );
    }
}

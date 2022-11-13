use super::context::Context;
use crate::{
    error::ApplicationError,
    hash_type::HashType,
    ir::{Build, Rule},
};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

const BUFFER_CAPACITY: usize = 2 << 10;

pub async fn calculate_timestamp_hash(
    context: &Context,
    build: &Build,
    file_inputs: &[&str],
    phony_inputs: &[&str],
) -> Result<u64, ApplicationError> {
    if let Some(hash) = calculate_phony_hash(build, file_inputs, phony_inputs) {
        return Ok(hash);
    }

    let mut hasher = DefaultHasher::new();

    hash_command(build, &mut hasher);

    for input in file_inputs {
        context
            .application()
            .file_system()
            .metadata(input.as_ref())
            .await?
            .modified_time()
            .hash(&mut hasher);
    }

    for &input in phony_inputs {
        get_build_hash(context, HashType::Timestamp, input)?.hash(&mut hasher);
    }

    Ok(hasher.finish())
}

pub async fn calculate_content_hash(
    context: &Context,
    build: &Build,
    file_inputs: &[&str],
    phony_inputs: &[&str],
) -> Result<u64, ApplicationError> {
    if let Some(hash) = calculate_phony_hash(build, file_inputs, phony_inputs) {
        return Ok(hash);
    }

    let mut hasher = DefaultHasher::new();

    hash_command(build, &mut hasher);

    let mut buffer = Vec::with_capacity(BUFFER_CAPACITY);

    for input in file_inputs {
        context
            .application()
            .file_system()
            .read_file(input.as_ref(), &mut buffer)
            .await?;
        buffer.hash(&mut hasher);
        buffer.clear();
    }

    for &input in phony_inputs {
        get_build_hash(context, HashType::Content, input)?.hash(&mut hasher);
    }

    Ok(hasher.finish())
}

fn get_build_hash(
    context: &Context,
    r#type: HashType,
    input: &str,
) -> Result<u64, ApplicationError> {
    context
        .application()
        .database()
        .get_hash(
            r#type,
            context
                .configuration()
                .outputs()
                .get(input)
                .ok_or_else(|| ApplicationError::InputNotFound(input.into()))?
                .id(),
        )?
        .ok_or_else(|| ApplicationError::InputNotBuilt(input.into()))
}

fn calculate_phony_hash(build: &Build, file_inputs: &[&str], phony_inputs: &[&str]) -> Option<u64> {
    if build.rule().is_none() && file_inputs.is_empty() && phony_inputs.is_empty() {
        Some(rand::random())
    } else {
        None
    }
}

fn hash_command(build: &Build, hasher: &mut impl Hasher) {
    build.rule().map(Rule::command).hash(hasher);
}

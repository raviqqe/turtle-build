use super::{build_hash::BuildHash, context::Context};
use crate::{
    error::InfrastructureError,
    ir::{Build, Rule},
};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::Path,
    time::SystemTime,
};
use tokio::{
    fs::{metadata, File},
    io::AsyncReadExt,
};

const BUFFER_CAPACITY: usize = 2 << 10;

pub async fn calculate_timestamp_hash<'a>(
    context: &Context<'a>,
    build: &Build<'a>,
    file_inputs: &[&'a str],
    phony_inputs: &[&'a str],
) -> Result<u64, InfrastructureError<'a>> {
    if let Some(hash) = calculate_fallback_hash(build, file_inputs, phony_inputs) {
        return Ok(hash);
    }

    let mut hasher = DefaultHasher::new();

    hash_command(build, &mut hasher);

    for input in file_inputs {
        read_timestamp(input).await?.hash(&mut hasher);
    }

    for &input in phony_inputs {
        get_build_hash(context, input)?
            .timestamp()
            .hash(&mut hasher);
    }

    Ok(hasher.finish())
}

pub async fn calculate_content_hash<'a>(
    context: &Context<'a>,
    build: &Build<'a>,
    file_inputs: &[&'a str],
    phony_inputs: &[&'a str],
) -> Result<u64, InfrastructureError<'a>> {
    if let Some(hash) = calculate_fallback_hash(build, file_inputs, phony_inputs) {
        return Ok(hash);
    }

    let mut hasher = DefaultHasher::new();

    hash_command(build, &mut hasher);

    let mut buffer = Vec::with_capacity(BUFFER_CAPACITY);

    for input in file_inputs {
        File::open(input).await?.read_to_end(&mut buffer).await?;
        buffer.hash(&mut hasher);
        buffer.clear();
    }

    for &input in phony_inputs {
        get_build_hash(context, input)?.content().hash(&mut hasher);
    }

    Ok(hasher.finish())
}

fn get_build_hash<'a>(
    context: &Context<'a>,
    input: &'a str,
) -> Result<BuildHash, InfrastructureError<'a>> {
    context
        .database()
        .get(
            context
                .configuration()
                .outputs()
                .get(input)
                .ok_or_else(|| InfrastructureError::InputNotFound(input.into()))?
                .id(),
        )?
        .ok_or_else(|| InfrastructureError::InputNotBuilt(input.into()))
}

fn calculate_fallback_hash(
    build: &Build,
    file_inputs: &[&str],
    phony_inputs: &[&str],
) -> Option<u64> {
    if build.rule().is_none() && file_inputs.is_empty() && phony_inputs.is_empty() {
        Some(rand::random())
    } else {
        None
    }
}

fn hash_command(build: &Build, hasher: &mut impl Hasher) {
    build.rule().map(Rule::command).hash(hasher);
}

async fn read_timestamp(path: impl AsRef<Path>) -> Result<SystemTime, InfrastructureError> {
    let path = path.as_ref();

    metadata(path)
        .await
        .map_err(|error| InfrastructureError::with_path(error, path))?
        .modified()
        .map_err(|error| InfrastructureError::with_path(error, path))
}

use crate::error::InfrastructureError;
use std::path::{Path, PathBuf};
use tokio::fs;

pub async fn canonicalize_path(path: impl AsRef<Path>) -> Result<PathBuf, InfrastructureError> {
    fs::canonicalize(&path)
        .await
        .map_err(|error| InfrastructureError::with_path(error, path))
}

use crate::error::InfrastructureError;
use std::path::{Path, PathBuf};
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
};

pub async fn read_file(path: impl AsRef<Path>) -> Result<String, InfrastructureError> {
    let mut source = "".into();
    let path = path.as_ref();

    File::open(path)
        .await
        .map_err(|error| InfrastructureError::with_path(error, path))?
        .read_to_string(&mut source)
        .await
        .map_err(|error| InfrastructureError::with_path(error, path))?;

    Ok(source)
}

pub async fn canonicalize_path(path: impl AsRef<Path>) -> Result<PathBuf, InfrastructureError> {
    fs::canonicalize(&path)
        .await
        .map_err(|error| InfrastructureError::with_path(error, path))
}

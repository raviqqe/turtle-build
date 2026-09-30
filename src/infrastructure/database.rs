mod error;
#[cfg(test)]
mod fake;
mod fjall;
mod log;
mod redb;

#[cfg(test)]
pub use self::fake::FakeDatabase;
pub use self::{error::DatabaseError, fjall::FjallDatabase, log::LogDatabase, redb::RedbDatabase};
use crate::{build_hash::BuildHash, ir::BuildId};
use alloc::sync::Arc;
use async_trait::async_trait;

/// A database.
#[async_trait]
pub trait Database {
    /// Gets a hash of a build.
    async fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError>;
    /// Sets a hash of a build.
    async fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError>;

    /// Gets header inputs of a build.
    async fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError>;
    /// Sets header inputs of a build.
    async fn set_header_inputs(
        &self,
        id: BuildId,
        inputs: &[Arc<str>],
    ) -> Result<(), DatabaseError>;

    /// Gets all outputs.
    async fn get_outputs(&self) -> Result<Vec<String>, DatabaseError>;
    /// Sets an output and its source in a source map.
    async fn set_output(&self, path: &str, source: Option<&str>) -> Result<(), DatabaseError>;
    /// Gets a source of an output in a source map.
    async fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError>;
}

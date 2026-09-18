mod error;
#[cfg(test)]
mod fake;
mod fjall;

#[cfg(test)]
pub use self::fake::FakeDatabase;
pub use self::{error::DatabaseError, fjall::FjallDatabase};
use crate::{hash_type::HashType, ir::BuildId};
use async_trait::async_trait;

#[async_trait]
pub trait Database {
    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, DatabaseError>;
    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), DatabaseError>;

    fn get_header_dependencies(&self, id: BuildId) -> Result<Vec<String>, DatabaseError>;
    fn set_header_dependencies(
        &self,
        id: BuildId,
        dependencies: &[String],
    ) -> Result<(), DatabaseError>;

    fn get_outputs(&self) -> Result<Vec<String>, DatabaseError>;
    fn set_output(&self, path: &str) -> Result<(), DatabaseError>;

    fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError>;
    fn set_source(&self, output: &str, source: &str) -> Result<(), DatabaseError>;

    async fn flush(&self) -> Result<(), DatabaseError>;
}

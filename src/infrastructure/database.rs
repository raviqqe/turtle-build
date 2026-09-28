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

pub trait Database {
    fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError>;
    fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError>;

    fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError>;
    fn set_header_inputs(&self, id: BuildId, inputs: &[Arc<str>]) -> Result<(), DatabaseError>;

    fn get_outputs(&self) -> Result<Vec<String>, DatabaseError>;
    fn set_output(&self, path: &str) -> Result<(), DatabaseError>;

    fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError>;
    fn set_source(&self, output: &str, source: &str) -> Result<(), DatabaseError>;
}

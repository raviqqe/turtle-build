mod fjall;

pub use self::fjall::FjallDatabase;
use crate::{hash_type::HashType, ir::BuildId};
use async_trait::async_trait;
use core::error::Error;
use std::path::Path;

#[async_trait]
pub trait Database {
    fn initialize(&self, path: &Path) -> Result<(), Box<dyn Error>>;

    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, Box<dyn Error>>;
    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), Box<dyn Error>>;

    fn get_header_dependencies(&self, id: BuildId) -> Result<Vec<String>, Box<dyn Error>>;
    fn set_header_dependencies(
        &self,
        id: BuildId,
        dependencies: &[String],
    ) -> Result<(), Box<dyn Error>>;

    fn get_outputs(&self) -> Result<Vec<String>, Box<dyn Error>>;
    fn set_output(&self, path: &str) -> Result<(), Box<dyn Error>>;

    fn get_source(&self, output: &str) -> Result<Option<String>, Box<dyn Error>>;
    fn set_source(&self, output: &str, source: &str) -> Result<(), Box<dyn Error>>;

    async fn flush(&self) -> Result<(), Box<dyn Error>>;
}

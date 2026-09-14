use crate::{hash_type::HashType, infrastructure::Database, ir::BuildId};
use async_trait::async_trait;
use std::{
    collections::{BTreeSet, HashMap},
    error::Error,
    path::Path,
    sync::Mutex,
};

#[derive(Debug, Default)]
pub struct FakeDatabase {
    timestamp_hashes: Mutex<HashMap<BuildId, u64>>,
    content_hashes: Mutex<HashMap<BuildId, u64>>,
    header_dependencies: Mutex<HashMap<BuildId, Vec<String>>>,
    outputs: Mutex<BTreeSet<String>>,
    sources: Mutex<HashMap<String, String>>,
}

impl FakeDatabase {
    fn hashes(&self, r#type: HashType) -> &Mutex<HashMap<BuildId, u64>> {
        match r#type {
            HashType::Content => &self.content_hashes,
            HashType::Timestamp => &self.timestamp_hashes,
        }
    }
}

#[async_trait]
impl Database for FakeDatabase {
    fn initialize(&self, _: &Path) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, Box<dyn Error>> {
        Ok(self.hashes(r#type).lock().unwrap().get(&id).copied())
    }

    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), Box<dyn Error>> {
        self.hashes(r#type).lock().unwrap().insert(id, hash);

        Ok(())
    }

    fn get_header_dependencies(&self, id: BuildId) -> Result<Vec<String>, Box<dyn Error>> {
        Ok(self
            .header_dependencies
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .unwrap_or_default())
    }

    fn set_header_dependencies(
        &self,
        id: BuildId,
        dependencies: &[String],
    ) -> Result<(), Box<dyn Error>> {
        self.header_dependencies
            .lock()
            .unwrap()
            .insert(id, dependencies.to_vec());

        Ok(())
    }

    fn get_outputs(&self) -> Result<Vec<String>, Box<dyn Error>> {
        Ok(self.outputs.lock().unwrap().iter().cloned().collect())
    }

    fn set_output(&self, path: &str) -> Result<(), Box<dyn Error>> {
        self.outputs.lock().unwrap().insert(path.into());

        Ok(())
    }

    fn get_source(&self, output: &str) -> Result<Option<String>, Box<dyn Error>> {
        Ok(self.sources.lock().unwrap().get(output).cloned())
    }

    fn set_source(&self, output: &str, source: &str) -> Result<(), Box<dyn Error>> {
        self.sources
            .lock()
            .unwrap()
            .insert(output.into(), source.into());

        Ok(())
    }

    async fn flush(&self) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

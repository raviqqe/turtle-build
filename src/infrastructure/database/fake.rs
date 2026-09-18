use crate::{
    hash_type::HashType,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use alloc::collections::BTreeSet;
use async_trait::async_trait;
use std::{collections::HashMap, sync::Mutex};

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
    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, DatabaseError> {
        Ok(self.hashes(r#type).lock().unwrap().get(&id).copied())
    }

    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), DatabaseError> {
        self.hashes(r#type).lock().unwrap().insert(id, hash);

        Ok(())
    }

    fn get_header_dependencies(&self, id: BuildId) -> Result<Vec<String>, DatabaseError> {
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
    ) -> Result<(), DatabaseError> {
        self.header_dependencies
            .lock()
            .unwrap()
            .insert(id, dependencies.to_vec());

        Ok(())
    }

    fn get_outputs(&self) -> Result<Vec<String>, DatabaseError> {
        Ok(self.outputs.lock().unwrap().iter().cloned().collect())
    }

    fn set_output(&self, path: &str) -> Result<(), DatabaseError> {
        self.outputs.lock().unwrap().insert(path.into());

        Ok(())
    }

    fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError> {
        Ok(self.sources.lock().unwrap().get(output).cloned())
    }

    fn set_source(&self, output: &str, source: &str) -> Result<(), DatabaseError> {
        self.sources
            .lock()
            .unwrap()
            .insert(output.into(), source.into());

        Ok(())
    }

    async fn flush(&self) -> Result<(), DatabaseError> {
        Ok(())
    }
}

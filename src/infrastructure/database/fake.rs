use crate::{
    hash_type::HashType,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use alloc::{collections::BTreeSet, sync::Arc};
use std::{collections::HashMap, sync::Mutex};

#[derive(Clone, Debug, Default)]
pub struct FakeDatabase {
    timestamp_hashes: Arc<Mutex<HashMap<BuildId, u64>>>,
    content_hashes: Arc<Mutex<HashMap<BuildId, u64>>>,
    header_dependencies: Arc<Mutex<HashMap<BuildId, Vec<String>>>>,
    header_dependency_requests: Arc<Mutex<Vec<BuildId>>>,
    outputs: Arc<Mutex<BTreeSet<String>>>,
    sources: Arc<Mutex<HashMap<String, String>>>,
}

impl FakeDatabase {
    pub fn header_dependency_requests(&self) -> Vec<BuildId> {
        self.header_dependency_requests.lock().unwrap().clone()
    }

    fn hashes(&self, r#type: HashType) -> &Mutex<HashMap<BuildId, u64>> {
        match r#type {
            HashType::Content => &self.content_hashes,
            HashType::Timestamp => &self.timestamp_hashes,
        }
    }
}

impl Database for FakeDatabase {
    fn get_hash(&self, r#type: HashType, id: BuildId) -> Result<Option<u64>, DatabaseError> {
        Ok(self.hashes(r#type).lock().unwrap().get(&id).copied())
    }

    fn set_hash(&self, r#type: HashType, id: BuildId, hash: u64) -> Result<(), DatabaseError> {
        self.hashes(r#type).lock().unwrap().insert(id, hash);

        Ok(())
    }

    fn get_header_dependencies(&self, id: BuildId) -> Result<Vec<String>, DatabaseError> {
        self.header_dependency_requests.lock().unwrap().push(id);

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
}

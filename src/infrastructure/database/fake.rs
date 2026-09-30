use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use alloc::{collections::BTreeMap, sync::Arc};
use async_trait::async_trait;
use std::{collections::HashMap, sync::Mutex};

#[derive(Clone, Debug, Default)]
pub struct FakeDatabase {
    hashes: Arc<Mutex<HashMap<BuildId, BuildHash>>>,
    header_dependencies: Arc<Mutex<HashMap<BuildId, Vec<Arc<str>>>>>,
    header_dependency_requests: Arc<Mutex<Vec<BuildId>>>,
    outputs: Arc<Mutex<BTreeMap<String, Option<String>>>>,
}

impl FakeDatabase {
    pub fn header_dependency_requests(&self) -> Vec<BuildId> {
        self.header_dependency_requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl Database for FakeDatabase {
    async fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError> {
        Ok(self.hashes.lock().unwrap().get(&id).copied())
    }

    async fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        self.hashes.lock().unwrap().insert(id, hash);

        Ok(())
    }

    async fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError> {
        self.header_dependency_requests.lock().unwrap().push(id);

        Ok(self
            .header_dependencies
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .unwrap_or_default())
    }

    async fn set_header_inputs(
        &self,
        id: BuildId,
        dependencies: &[Arc<str>],
    ) -> Result<(), DatabaseError> {
        self.header_dependencies
            .lock()
            .unwrap()
            .insert(id, dependencies.to_vec());

        Ok(())
    }

    async fn get_outputs(&self) -> Result<Vec<String>, DatabaseError> {
        Ok(self.outputs.lock().unwrap().keys().cloned().collect())
    }

    async fn set_output(&self, path: &str, source: Option<&str>) -> Result<(), DatabaseError> {
        self.outputs
            .lock()
            .unwrap()
            .insert(path.into(), source.map(From::from));

        Ok(())
    }

    async fn get_source(&self, output: &str) -> Result<Option<String>, DatabaseError> {
        Ok(self.outputs.lock().unwrap().get(output).cloned().flatten())
    }
}

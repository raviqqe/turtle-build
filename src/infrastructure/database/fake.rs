use crate::{
    build_hash::BuildHash,
    infrastructure::{Database, DatabaseError},
    ir::BuildId,
};
use alloc::{collections::BTreeSet, sync::Arc};
use std::{collections::HashMap, sync::Mutex};

#[derive(Clone, Debug, Default)]
pub struct FakeDatabase {
    hashes: Arc<Mutex<HashMap<BuildId, BuildHash>>>,
    header_dependencies: Arc<Mutex<HashMap<BuildId, Vec<Arc<str>>>>>,
    header_dependency_requests: Arc<Mutex<Vec<BuildId>>>,
    outputs: Arc<Mutex<BTreeSet<String>>>,
    sources: Arc<Mutex<HashMap<String, String>>>,
}

impl FakeDatabase {
    pub fn header_dependency_requests(&self) -> Vec<BuildId> {
        self.header_dependency_requests.lock().unwrap().clone()
    }
}

impl Database for FakeDatabase {
    fn get_hash(&self, id: BuildId) -> Result<Option<BuildHash>, DatabaseError> {
        Ok(self.hashes.lock().unwrap().get(&id).copied())
    }

    fn set_hash(&self, id: BuildId, hash: BuildHash) -> Result<(), DatabaseError> {
        self.hashes.lock().unwrap().insert(id, hash);

        Ok(())
    }

    fn get_header_inputs(&self, id: BuildId) -> Result<Vec<Arc<str>>, DatabaseError> {
        self.header_dependency_requests.lock().unwrap().push(id);

        Ok(self
            .header_dependencies
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .unwrap_or_default())
    }

    fn set_header_inputs(
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

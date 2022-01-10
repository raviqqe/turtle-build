use super::error::InfrastructureError;
use std::path::Path;

const DATABASE_FILENAME: &str = ".turtle-sled-db";

#[derive(Clone, Debug)]
pub struct BuildDatabase {
    database: sled::Db,
}

impl BuildDatabase {
    pub fn new(build_directory: &Path) -> Result<Self, InfrastructureError> {
        Ok(Self {
            database: sled::open(build_directory.join(DATABASE_FILENAME))?,
        })
    }

    pub fn get(&self, id: &str) -> Result<u64, InfrastructureError> {
        Ok(if let Some(value) = self.database.get(id)? {
            u64::from_le_bytes(value.as_ref().try_into().unwrap())
        } else {
            0
        })
    }

    pub fn set(&self, id: &str, hash: u64) -> Result<(), InfrastructureError> {
        self.database.insert(id, &hash.to_le_bytes())?;

        Ok(())
    }
}

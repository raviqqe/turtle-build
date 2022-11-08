use super::build_hash::BuildHash;
use crate::{error::InfrastructureError, ir::BuildId};
use std::path::Path;

const DATABASE_FILENAME: &str = ".turtle-sled-db";

#[derive(Clone, Debug)]
pub struct BuildDatabase {
    database: sled::Db,
}

impl BuildDatabase {
    pub fn new(build_directory: &Path) -> Result<Self, InfrastructureError<'static>> {
        Ok(Self {
            database: sled::open(build_directory.join(DATABASE_FILENAME))?,
        })
    }

    pub fn get(&self, id: BuildId) -> Result<Option<BuildHash>, InfrastructureError<'static>> {
        Ok(self
            .database
            .get(id.to_bytes())?
            .map(|value| bincode::deserialize(&value))
            .transpose()?)
    }

    pub fn set(&self, id: BuildId, hash: BuildHash) -> Result<(), InfrastructureError<'static>> {
        self.database
            .insert(id.to_bytes(), bincode::serialize(&hash)?)?;

        Ok(())
    }

    pub async fn flush(&self) -> Result<(), InfrastructureError<'static>> {
        self.database.flush_async().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn open_database() {
        BuildDatabase::new(tempdir().unwrap().path()).unwrap();
    }

    #[test]
    fn set_hash() {
        let database = BuildDatabase::new(tempdir().unwrap().path()).unwrap();

        database.set(BuildId::new(0), BuildHash::new(0, 0)).unwrap();
    }

    #[test]
    fn get_hash() {
        let database = BuildDatabase::new(tempdir().unwrap().path()).unwrap();
        let hash = BuildHash::new(0, 1);

        database.set(BuildId::new(0), hash).unwrap();
        assert_eq!(database.get(BuildId::new(0)).unwrap(), Some(hash));
    }
}

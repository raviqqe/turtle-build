use super::build_hash::BuildHash;
use crate::error::InfrastructureError;
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

    pub fn get(&self, id: &str) -> Result<Option<BuildHash>, InfrastructureError> {
        Ok(self
            .database
            .get(id)?
            .map(|value| bincode::deserialize(value.as_ref()))
            .transpose()?)
    }

    pub fn set(&self, id: &str, hash: BuildHash) -> Result<(), InfrastructureError> {
        self.database.insert(id, bincode::serialize(&hash)?)?;

        Ok(())
    }

    pub async fn flush(&self) -> Result<(), InfrastructureError> {
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

        database.set("foo", BuildHash::new(0, 0)).unwrap();
    }

    #[test]
    fn get_hash() {
        let database = BuildDatabase::new(tempdir().unwrap().path()).unwrap();
        let hash = BuildHash::new(0, 1);

        database.set("foo", hash).unwrap();
        assert_eq!(database.get("foo").unwrap(), Some(hash));
    }
}

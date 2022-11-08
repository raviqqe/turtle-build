use super::build_hash::BuildHash;
use crate::error::InfrastructureError;
use std::path::Path;

const DATABASE_FILENAME: &str = ".turtle-sled-db";

#[derive(Clone, Debug)]
pub struct BuildDatabase {
    database: sled::Db,
}

impl BuildDatabase {
    pub fn new<'a>(build_directory: &Path) -> Result<Self, InfrastructureError<'a>> {
        Ok(Self {
            database: sled::open(build_directory.join(DATABASE_FILENAME))?,
        })
    }

    pub fn get<'a>(&self, id: u64) -> Result<Option<BuildHash>, InfrastructureError<'a>> {
        Ok(self
            .database
            .get(id.to_le_bytes())?
            .map(|value| bincode::deserialize(&value))
            .transpose()?)
    }

    pub fn set<'a>(&self, id: u64, hash: BuildHash) -> Result<(), InfrastructureError<'a>> {
        self.database
            .insert(id.to_le_bytes(), bincode::serialize(&hash)?)?;

        Ok(())
    }

    pub async fn flush<'a>(&self) -> Result<(), InfrastructureError<'a>> {
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

        database.set(0, BuildHash::new(0, 0)).unwrap();
    }

    #[test]
    fn get_hash() {
        let database = BuildDatabase::new(tempdir().unwrap().path()).unwrap();
        let hash = BuildHash::new(0, 1);

        database.set(0, hash).unwrap();
        assert_eq!(database.get(0).unwrap(), Some(hash));
    }
}

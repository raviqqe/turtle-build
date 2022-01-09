use super::error::RunError;
use std::path::Path;

const DEFAULT_DATABASE_FILENAME: &str = ".ninja_deps";

#[derive(Debug)]
pub struct BuildDatabase {
    database: sled::Db,
}

impl BuildDatabase {
    pub fn new(build_directory: &Path) -> Result<Self, RunError> {
        Ok(Self {
            database: sled::open(build_directory.join(DEFAULT_DATABASE_FILENAME))?,
        })
    }

    pub fn get(&self, path: &Path) -> Result<u64, RunError> {
        Ok(
            if let Some(value) = self.database.get(Self::get_key(path))? {
                u64::from_le_bytes(value.as_ref().try_into().unwrap())
            } else {
                0
            },
        )
    }

    pub fn set(&self, path: &Path, hash: u64) -> Result<(), RunError> {
        self.database
            .insert(Self::get_key(path), &hash.to_le_bytes())?;

        Ok(())
    }

    fn get_key(path: &Path) -> impl AsRef<[u8]> {
        format!("{}", path.display())
    }
}

use crate::{
    ast::{Module, Statement},
    error::BuildError,
    file::canonicalize_native_path,
    infrastructure::FileSystem,
    parse::parse,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Loads a root module and its dependency modules.
pub async fn load(
    file_system: &impl FileSystem,
    path: &Path,
) -> Result<HashMap<PathBuf, Module>, BuildError> {
    let mut paths = vec![(path.to_path_buf(), vec![])];
    let mut modules = HashMap::new();

    while let Some((path, mut ancestors)) = paths.pop() {
        let canonical_path = canonicalize_native_path(&path);

        if ancestors.contains(&canonical_path) {
            return Err(BuildError::CircularModuleDependency);
        } else if modules.contains_key(&canonical_path) {
            continue;
        }

        let module = parse(&file_system.read_file_to_string(&path).await?)?;

        ancestors.push(canonical_path.clone());
        paths.extend(
            module
                .statements()
                .iter()
                .filter_map(|statement| match statement {
                    Statement::Include(include) => Some(include.path()),
                    Statement::Submodule(submodule) => Some(submodule.path()),
                    _ => None,
                })
                // TODO Interpolate variables in paths of included and subninja files like ninja.
                .map(|path| (path.into(), ancestors.clone())),
        );

        modules.insert(canonical_path, module);
    }

    Ok(modules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::{FakeFileSystem, FileError};
    use pretty_assertions::assert_eq;

    fn create_modules(sources: &[(&str, &str)]) -> HashMap<PathBuf, Module> {
        sources
            .iter()
            .map(|(path, source)| (PathBuf::from(path), parse(source).unwrap()))
            .collect()
    }

    fn create_file_system(sources: &[(&str, &str)]) -> FakeFileSystem {
        let file_system = FakeFileSystem::default();

        for (path, source) in sources {
            file_system.write_file(path, source);
        }

        file_system
    }

    #[tokio::test]
    async fn load_root_module() {
        let sources = [("build.ninja", "x = 42\n")];

        assert_eq!(
            load(&create_file_system(&sources), Path::new("build.ninja")).await,
            Ok(create_modules(&sources))
        );
    }

    #[tokio::test]
    async fn load_included_module() {
        let sources = [
            ("build.ninja", "include foo.ninja\n"),
            ("foo.ninja", "x = 42\n"),
        ];

        assert_eq!(
            load(&create_file_system(&sources), Path::new("build.ninja")).await,
            Ok(create_modules(&sources))
        );
    }

    #[tokio::test]
    async fn load_child_module() {
        let sources = [
            ("build.ninja", "subninja foo.ninja\n"),
            ("foo.ninja", "x = 42\n"),
        ];

        assert_eq!(
            load(&create_file_system(&sources), Path::new("build.ninja")).await,
            Ok(create_modules(&sources))
        );
    }

    #[tokio::test]
    async fn load_module_included_from_two_modules() {
        let sources = [
            ("build.ninja", "include foo.ninja\ninclude bar.ninja\n"),
            ("foo.ninja", "include baz.ninja\n"),
            ("bar.ninja", "subninja baz.ninja\n"),
            ("baz.ninja", "x = 42\n"),
        ];
        let file_system = create_file_system(&sources);

        assert_eq!(
            load(&file_system, Path::new("build.ninja")).await,
            Ok(create_modules(&sources))
        );
        assert_eq!(
            file_system
                .read_requests()
                .iter()
                .filter(|path| path == &Path::new("baz.ninja"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn canonicalize_root_module_path() {
        let source = "x = 42\n";
        let file_system = create_file_system(&[("./build.ninja", source)]);

        assert_eq!(
            load(&file_system, Path::new("./build.ninja")).await,
            Ok(create_modules(&[("build.ninja", source)]))
        );
        assert_eq!(
            file_system.read_requests(),
            [PathBuf::from("./build.ninja")]
        );
    }

    #[tokio::test]
    async fn canonicalize_included_module_paths() {
        let root_source = "include ./foo.ninja\nsubninja bar/../foo.ninja\n";
        let source = "x = 42\n";
        let file_system = create_file_system(&[
            ("build.ninja", root_source),
            ("./foo.ninja", source),
            ("bar/../foo.ninja", source),
        ]);

        assert_eq!(
            load(&file_system, Path::new("build.ninja")).await,
            Ok(create_modules(&[
                ("build.ninja", root_source),
                ("foo.ninja", source)
            ]))
        );
        assert_eq!(file_system.read_requests().len(), 2);
    }

    #[tokio::test]
    async fn resolve_module_path_relative_to_working_directory() {
        let sources = [
            ("build.ninja", "include foo/foo.ninja\n"),
            ("foo/foo.ninja", "include bar.ninja\n"),
            ("bar.ninja", "x = 42\n"),
        ];

        assert_eq!(
            load(&create_file_system(&sources), Path::new("build.ninja")).await,
            Ok(create_modules(&sources))
        );
    }

    #[tokio::test]
    async fn fail_to_read_module() {
        assert_eq!(
            load(
                &create_file_system(&[("build.ninja", "include foo.ninja\n")]),
                Path::new("build.ninja")
            )
            .await,
            Err(FileError::new("file not found").into())
        );
    }

    #[tokio::test]
    async fn fail_to_read_module_in_missing_directory() {
        assert_eq!(
            load(
                &create_file_system(&[
                    ("build.ninja", "include bar/../foo.ninja\n"),
                    ("foo.ninja", "x = 42\n"),
                ]),
                Path::new("build.ninja")
            )
            .await,
            Err(FileError::new("file not found").into())
        );
    }

    #[tokio::test]
    async fn fail_to_include_module_in_itself() {
        assert_eq!(
            load(
                &create_file_system(&[("build.ninja", "include build.ninja\n")]),
                Path::new("build.ninja")
            )
            .await,
            Err(BuildError::CircularModuleDependency)
        );
    }

    #[tokio::test]
    async fn fail_to_include_modules_in_each_other() {
        assert_eq!(
            load(
                &create_file_system(&[
                    ("build.ninja", "subninja foo.ninja\n"),
                    ("foo.ninja", "subninja ./build.ninja\n"),
                ]),
                Path::new("build.ninja")
            )
            .await,
            Err(BuildError::CircularModuleDependency)
        );
    }
}

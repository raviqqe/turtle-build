use crate::{
    ast::{Module, Statement},
    compile::ModuleDependencyMap,
    context::Context,
    error::ApplicationError,
    parse::parse,
};
use futures::future::try_join_all;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub async fn parse_modules(
    context: &Context,
    path: &Path,
) -> Result<(HashMap<PathBuf, Module>, ModuleDependencyMap), ApplicationError> {
    let mut paths = vec![context.file_system().canonicalize_path(path).await?];
    let mut modules = HashMap::new();
    let mut dependencies = HashMap::new();

    while let Some(path) = paths.pop() {
        let mut source = String::new();

        context
            .file_system()
            .read_file_to_string(&path, &mut source)
            .await?;

        let module = parse(&source)?;

        let submodule_paths = try_join_all(
            module
                .statements()
                .iter()
                .filter_map(|statement| match statement {
                    Statement::Include(include) => Some(include.path()),
                    Statement::Submodule(submodule) => Some(submodule.path()),
                    _ => None,
                })
                .map(|submodule_path| resolve_submodule_path(context, &path, submodule_path))
                .collect::<Vec<_>>(),
        )
        .await?
        .into_iter()
        .collect::<HashMap<_, _>>();

        paths.extend(submodule_paths.values().cloned());

        modules.insert(path.clone(), module);
        dependencies.insert(path, submodule_paths);
    }

    Ok((modules, dependencies))
}

async fn resolve_submodule_path(
    context: &Context,
    module_path: &Path,
    submodule_path: &str,
) -> Result<(String, PathBuf), ApplicationError> {
    Ok((
        submodule_path.into(),
        context
            .file_system()
            .canonicalize_path(&module_path.parent().unwrap().join(submodule_path))
            .await?,
    ))
}

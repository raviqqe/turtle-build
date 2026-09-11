use std::path::{Component, Path, PathBuf};

// A pure, lexical equivalent of ninja's `CanonicalizePath`. It MUST NOT TOUCH
// the file system (no symlink resolution, no existence check), since it also
// applies to paths for files that may not exist yet (like... generated
// headers).
pub fn canonicalize_path(path: &str) -> String {
    let path = Path::new(path)
        .components()
        .fold(vec![], |mut components, component| {
            match (components.last(), component) {
                (_, Component::CurDir) => {}
                (Some(Component::Normal(_)), Component::ParentDir) => {
                    components.pop();
                }
                (Some(Component::Prefix(_) | Component::RootDir), Component::ParentDir) => {}
                _ => components.push(component),
            }

            components
        })
        .into_iter()
        .collect::<PathBuf>();

    if path.as_os_str().is_empty() {
        ".".into()
    } else {
        path.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn canonicalize_current_directory_component() {
        assert_eq!(canonicalize_path("foo/./bar"), "foo/bar");
        assert_eq!(canonicalize_path("./foo.c"), "foo.c");
    }

    #[test]
    fn canonicalize_parent_directory_component() {
        assert_eq!(canonicalize_path("a/b/../c"), "a/c");
        assert_eq!(canonicalize_path("a/.."), ".");
        assert_eq!(canonicalize_path("a/../.."), "..");
    }

    #[test]
    fn canonicalize_leading_parent_directory_component() {
        assert_eq!(canonicalize_path("../up.h"), "../up.h");
        assert_eq!(canonicalize_path("../../a"), "../../a");
    }

    #[test]
    fn canonicalize_duplicate_slashes() {
        assert_eq!(canonicalize_path("a//b.h"), "a/b.h");
    }

    #[test]
    fn canonicalize_trailing_slash() {
        assert_eq!(canonicalize_path("a/b/"), "a/b");
    }

    #[test]
    fn canonicalize_absolute_path() {
        assert_eq!(
            canonicalize_path("/usr/include/stdio.h"),
            "/usr/include/stdio.h"
        );
        assert_eq!(
            canonicalize_path("//usr/include/stdio.h"),
            "/usr/include/stdio.h"
        );
        assert_eq!(canonicalize_path("/../usr/include"), "/usr/include");
    }

    #[test]
    fn canonicalize_empty_path() {
        assert_eq!(canonicalize_path(""), ".");
        assert_eq!(canonicalize_path("."), ".");
    }

    #[test]
    fn canonicalize_unchanged_path() {
        assert_eq!(canonicalize_path("foo.c"), "foo.c");
    }
}

use std::path::{Component, MAIN_SEPARATOR, Path, PathBuf};

/// Canonicalizes a path lexically.
pub fn canonicalize_path(path: &str) -> String {
    // Paths are canonicalized lexically like ninja does, and never through the
    // file system, because they may point to files which do not exist yet, like
    // generated headers.
    canonicalize_native_path(path.as_ref())
        .to_string_lossy()
        .replace(MAIN_SEPARATOR, "/")
}

pub fn canonicalize_native_path(path: &Path) -> PathBuf {
    let path = path
        .components()
        .fold(vec![], |mut components, component| {
            match (components.last(), component) {
                (_, Component::CurDir) => {}
                (Some(Component::Normal(_)), Component::ParentDir) => {
                    components.pop();
                }
                (Some(Component::RootDir), Component::ParentDir) => {}
                _ => components.push(component),
            }

            components
        })
        .into_iter()
        .collect::<PathBuf>();

    if path.as_os_str().is_empty() {
        ".".into()
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn canonicalize_empty_path() {
        assert_eq!(canonicalize_path(""), ".");
        assert_eq!(canonicalize_path("."), ".");
    }

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
        assert_eq!(canonicalize_path("/../usr/include"), "/usr/include");
        assert_eq!(canonicalize_path("/"), "/");
    }

    #[test]
    fn canonicalize_unchanged_path() {
        assert_eq!(canonicalize_path("foo.c"), "foo.c");
    }

    #[test]
    fn canonicalize_native_empty_path() {
        assert_eq!(canonicalize_native_path(Path::new("")), Path::new("."));
    }

    #[test]
    fn canonicalize_native_relative_path() {
        assert_eq!(
            canonicalize_native_path(Path::new("./a/b/../c")),
            Path::new("a/c")
        );
    }

    #[cfg(unix)]
    #[test]
    fn canonicalize_unix_path() {
        assert_eq!(
            canonicalize_path("//usr/include/stdio.h"),
            "/usr/include/stdio.h"
        );
    }

    #[cfg(windows)]
    #[test]
    fn canonicalize_windows_path() {
        assert_eq!(canonicalize_path("a\\b/../c"), "a/c");
        assert_eq!(canonicalize_path("C:\\a\\..\\b"), "C:/b");
        assert_eq!(canonicalize_path("C:..\\a"), "C:../a");
        assert_eq!(canonicalize_path("\\a\\b"), "/a/b");
        assert_eq!(
            canonicalize_path("\\\\server\\share\\a\\..\\b"),
            "//server/share/b"
        );
        assert_eq!(
            canonicalize_path("//server/share/a/../b"),
            "//server/share/b"
        );
    }
}

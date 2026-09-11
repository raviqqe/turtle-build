// A pure, lexical equivalent of ninja's `CanonicalizePath`. It MUST NOT TOUCH
// the file system (no symlink resolution, no existence check), since it also
// applies to paths for files that may not exist yet (like... generated
// headers).
pub fn canonicalize_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut components: Vec<&str> = vec![];

    for component in path.split('/') {
        if component.is_empty() || component == "." {
            continue;
        } else if component == ".." {
            if matches!(components.last(), Some(&last) if last != "..") {
                components.pop();
            } else if !absolute {
                components.push("..");
            }
        } else {
            components.push(component);
        }
    }

    let joined = components.join("/");

    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".into()
    } else {
        joined
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
    }

    #[test]
    fn canonicalize_unchanged_path() {
        assert_eq!(canonicalize_path("foo.c"), "foo.c");
    }
}

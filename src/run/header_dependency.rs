use super::context::Context;
use crate::{
    error::ApplicationError,
    file::canonicalize_path,
    ir::{HeaderDependency, Rule},
    parse::parse_depfile,
};
use std::{borrow::Cow, process::Output};

pub async fn read_header_dependencies(
    context: &Context,
    rule: &Rule,
    output: &Output,
) -> Result<Vec<String>, ApplicationError> {
    let dependencies = match rule.header_dependency() {
        None => vec![],
        Some(HeaderDependency::Make { path } | HeaderDependency::Gcc { path }) => {
            read_depfile(context, path).await?
        }
        Some(HeaderDependency::Msvc { prefix }) => {
            extract_show_includes(&output.stdout, prefix.as_bytes())
        }
    };

    if let Some(HeaderDependency::Gcc { path }) = rule.header_dependency()
        && context
            .application()
            .file_system()
            .exists(path.as_ref())
            .await?
    {
        context
            .application()
            .file_system()
            .remove_file(path.as_ref())
            .await?;
    }

    Ok(dependencies
        .into_iter()
        .map(|path| canonicalize_path(&path))
        .collect())
}

pub fn exclude_show_includes<'a>(rule: &Rule, output: &'a [u8]) -> Cow<'a, [u8]> {
    if let Some(HeaderDependency::Msvc { prefix }) = rule.header_dependency() {
        output
            .split(|&byte| byte == b'\n')
            .filter(|line| !line.starts_with(prefix.as_bytes()))
            .collect::<Vec<_>>()
            .join(&b'\n')
            .into()
    } else {
        output.into()
    }
}

async fn read_depfile(context: &Context, path: &str) -> Result<Vec<String>, ApplicationError> {
    if !context
        .application()
        .file_system()
        .exists(path.as_ref())
        .await?
    {
        return Ok(vec![]);
    }

    let mut source = String::new();

    context
        .application()
        .file_system()
        .read_file_to_string(path.as_ref(), &mut source)
        .await?;

    Ok(parse_depfile(&source)?)
}

fn extract_show_includes(output: &[u8], prefix: &[u8]) -> Vec<String> {
    output
        .split(|&byte| byte == b'\n')
        .filter_map(|line| line.strip_prefix(prefix))
        .map(|include| String::from_utf8_lossy(include.trim_ascii()).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn msvc_rule(prefix: &str) -> Rule {
        Rule::new("", None).with_header_dependency(Some(HeaderDependency::Msvc {
            prefix: prefix.into(),
        }))
    }

    #[test]
    fn extract_show_includes_with_no_output() {
        assert_eq!(
            extract_show_includes(b"", b"Note: including file: "),
            Vec::<String>::new()
        );
    }

    #[test]
    fn extract_show_includes_with_leading_blank_line() {
        assert_eq!(
            extract_show_includes(b"\nAAA\n\nBBB\n", b"Note: including file: "),
            Vec::<String>::new()
        );
    }

    #[test]
    fn extract_show_includes_with_only_includes() {
        assert_eq!(
            extract_show_includes(
                b"Note: including file: foo.h\nNote: including file: bar.h\n",
                b"Note: including file: "
            ),
            vec!["foo.h", "bar.h"]
        );
    }

    #[test]
    fn extract_show_includes_interleaved_with_output() {
        assert_eq!(
            extract_show_includes(
                b"AAA\nNote: including file: foo.h\nBBB\n",
                b"Note: including file: "
            ),
            vec!["foo.h"]
        );
    }

    #[test]
    fn extract_show_includes_with_windows_line_endings() {
        assert_eq!(
            extract_show_includes(
                b"Note: including file: foo.h\r\nAAA\r\n",
                b"Note: including file: "
            ),
            vec!["foo.h"]
        );
    }

    #[test]
    fn extract_show_includes_with_indented_include() {
        assert_eq!(
            extract_show_includes(b"Note: including file:  foo.h\n", b"Note: including file: "),
            vec!["foo.h"]
        );
    }

    #[test]
    fn extract_show_includes_with_custom_prefix() {
        assert_eq!(
            extract_show_includes(b"Hinweis: foo.h\n", b"Hinweis: "),
            vec!["foo.h"]
        );
    }

    #[test]
    fn exclude_show_includes_without_msvc_header_dependency() {
        assert_eq!(
            exclude_show_includes(&Rule::new("", None), b"Note: including file: foo.h\nAAA\n"),
            b"Note: including file: foo.h\nAAA\n".to_vec()
        );
    }

    #[test]
    fn exclude_show_includes_with_no_output() {
        assert_eq!(
            exclude_show_includes(&msvc_rule("Note: including file: "), b""),
            b"".to_vec()
        );
    }

    #[test]
    fn exclude_show_includes_with_leading_blank_line() {
        assert_eq!(
            exclude_show_includes(&msvc_rule("Note: including file: "), b"\nAAA\n\nBBB\n"),
            b"\nAAA\n\nBBB\n".to_vec()
        );
    }

    #[test]
    fn exclude_show_includes_with_only_includes() {
        assert_eq!(
            exclude_show_includes(
                &msvc_rule("Note: including file: "),
                b"Note: including file: foo.h\nNote: including file: bar.h\n"
            ),
            b"".to_vec()
        );
    }

    #[test]
    fn exclude_show_includes_interleaved_with_output() {
        assert_eq!(
            exclude_show_includes(
                &msvc_rule("Note: including file: "),
                b"AAA\nNote: including file: foo.h\nBBB\n"
            ),
            b"AAA\nBBB\n".to_vec()
        );
    }

    #[test]
    fn exclude_show_includes_with_windows_line_endings() {
        assert_eq!(
            exclude_show_includes(
                &msvc_rule("Note: including file: "),
                b"Note: including file: foo.h\r\nAAA\r\n"
            ),
            b"AAA\r\n".to_vec()
        );
    }

    #[test]
    fn exclude_show_includes_with_indented_include() {
        assert_eq!(
            exclude_show_includes(
                &msvc_rule("Note: including file: "),
                b"Note: including file:  foo.h\n"
            ),
            b"".to_vec()
        );
    }

    #[test]
    fn exclude_show_includes_with_custom_prefix() {
        assert_eq!(
            exclude_show_includes(&msvc_rule("Hinweis: "), b"Hinweis: foo.h\n"),
            b"".to_vec()
        );
    }
}

use super::context::Context;
use crate::{
    error::ApplicationError,
    file::canonicalize_path,
    ir::{HeaderDependency, Rule},
    parse::parse_depfile,
};
use std::process::Output;

pub async fn read_rule_output(
    context: &Context,
    rule: &Rule,
    output: &mut Output,
) -> Result<Vec<String>, ApplicationError> {
    let mut dependencies = match rule.header_dependency() {
        None => vec![],
        Some(HeaderDependency::Depfile { path } | HeaderDependency::Gcc { path }) => {
            read_depfile(context, path).await?
        }
        Some(HeaderDependency::Msvc { prefix }) => {
            let (includes, stdout) = extract_show_includes(&output.stdout, prefix.as_bytes());

            output.stdout = stdout;

            includes
        }
    };

    for dependency in &mut dependencies {
        *dependency = canonicalize_path(dependency);
    }

    Ok(dependencies)
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

fn extract_show_includes(output: &[u8], prefix: &[u8]) -> (Vec<String>, Vec<u8>) {
    let mut filtered_output = vec![];
    let mut includes = vec![];
    let mut first_line = true;

    for line in output.split(|&byte| byte == b'\n') {
        if let Some(include) = line.strip_prefix(prefix) {
            includes.push(String::from_utf8_lossy(include.trim_ascii()).into_owned());
        } else {
            if !first_line {
                filtered_output.push(b'\n');
            }

            first_line = false;

            filtered_output.extend_from_slice(line);
        }
    }

    (includes, filtered_output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn extract_show_includes_with_no_output() {
        assert_eq!(
            extract_show_includes(b"", b"Note: including file: "),
            (vec![], vec![])
        );
    }

    #[test]
    fn extract_show_includes_with_leading_blank_line() {
        assert_eq!(
            extract_show_includes(b"\nAAA\n\nBBB\n", b"Note: including file: "),
            (vec![], b"\nAAA\n\nBBB\n".to_vec())
        );
    }

    #[test]
    fn extract_show_includes_with_only_includes() {
        assert_eq!(
            extract_show_includes(
                b"Note: including file: foo.h\nNote: including file: bar.h\n",
                b"Note: including file: "
            ),
            (vec!["foo.h".into(), "bar.h".into()], vec![])
        );
    }

    #[test]
    fn extract_show_includes_interleaved_with_output() {
        assert_eq!(
            extract_show_includes(
                b"AAA\nNote: including file: foo.h\nBBB\n",
                b"Note: including file: "
            ),
            (vec!["foo.h".into()], b"AAA\nBBB\n".to_vec())
        );
    }

    #[test]
    fn extract_show_includes_with_windows_line_endings() {
        assert_eq!(
            extract_show_includes(
                b"Note: including file: foo.h\r\nAAA\r\n",
                b"Note: including file: "
            ),
            (vec!["foo.h".into()], b"AAA\r\n".to_vec())
        );
    }

    #[test]
    fn extract_show_includes_with_indented_include() {
        assert_eq!(
            extract_show_includes(b"Note: including file:  foo.h\n", b"Note: including file: "),
            (vec!["foo.h".into()], vec![])
        );
    }

    #[test]
    fn extract_show_includes_with_custom_prefix() {
        assert_eq!(
            extract_show_includes(b"Hinweis: foo.h\n", b"Hinweis: "),
            (vec!["foo.h".into()], vec![])
        );
    }
}

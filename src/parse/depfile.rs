use super::ParseError;
use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::tag,
    character::complete::{line_ending, multispace1, none_of, one_of, space1},
    combinator::{all_consuming, cut, eof, map, not, peek, value},
    multi::{many0, many0_count, many1},
    sequence::{preceded, terminated},
};

// cspell: ignore multispace

pub fn parse_depfile(path: &str, source: &str) -> Result<Vec<String>, ParseError> {
    depfile(source)
        .map(|(_, dependencies)| dependencies)
        .map_err(|error| {
            // A colon after the targets of a rule is the only thing the grammar
            // can fail to find.
            let remaining = match error {
                nom::Err::Error(error) | nom::Err::Failure(error) => error.input,
                nom::Err::Incomplete(_) => "",
            };
            let consumed = &source[..source.len() - remaining.len()];
            let line = consumed
                .rsplit_once('\n')
                .map_or(consumed, |(_, line)| line);

            ParseError::new(format!(
                "{path}:{}:{}: expected ':'",
                consumed.matches('\n').count() + 1,
                line.chars().count() + 1
            ))
        })
}

fn depfile(input: &str) -> IResult<&str, Vec<String>> {
    map(
        all_consuming(preceded(blank, many0(terminated(rule, blank)))),
        |rules| rules.into_iter().flatten().collect(),
    )
    .parse(input)
}

// Dependencies of every rule are collected regardless of its targets, which
// only ever over-approximates the dependencies of a build.
fn rule(input: &str) -> IResult<&str, Vec<String>> {
    map(
        (
            not(eof),
            inline_blank,
            many0(terminated(token, inline_blank)),
            cut(colon),
            terminated(many0(preceded(inline_blank, token)), inline_blank),
        ),
        |(_, _, _, _, dependencies)| dependencies,
    )
    .parse(input)
}

fn token(input: &str) -> IResult<&str, String> {
    map(many1(token_character), |characters| {
        characters.into_iter().collect()
    })
    .parse(input)
}

fn token_character(input: &str) -> IResult<&str, char> {
    alt((
        preceded(tag("\\"), one_of(" \t:#")),
        value('\\', terminated(tag("\\"), not(one_of("\r\n")))),
        value('$', tag("$$")),
        value(':', terminated(tag(":"), peek(none_of(" \t\r\n")))),
        none_of(" \t\r\n\\:"),
    ))
    .parse(input)
}

fn colon(input: &str) -> IResult<&str, ()> {
    value(
        (),
        terminated(
            tag(":"),
            peek(alt((value((), one_of(" \t\r\n")), value((), eof)))),
        ),
    )
    .parse(input)
}

fn inline_blank(input: &str) -> IResult<&str, ()> {
    value(
        (),
        many0_count(alt((value((), space1), value((), (tag("\\"), newline))))),
    )
    .parse(input)
}

fn newline(input: &str) -> IResult<&str, &str> {
    alt((line_ending, tag("\r"))).parse(input)
}

fn blank(input: &str) -> IResult<&str, ()> {
    value((), many0_count(alt((value((), multispace1), comment)))).parse(input)
}

fn comment(input: &str) -> IResult<&str, ()> {
    value((), (tag("#"), many0_count(none_of("\n")))).parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parse_depfile(source: &str) -> Result<Vec<String>, ParseError> {
        super::parse_depfile("foo.d", source)
    }

    #[test]
    fn parse_empty() {
        assert_eq!(parse_depfile("").unwrap(), Vec::<String>::new());
        assert_eq!(parse_depfile("\n \t\r\n").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn parse_dependencies() {
        assert_eq!(
            parse_depfile("foo.o: foo.c foo.h\n").unwrap(),
            vec!["foo.c", "foo.h"]
        );
        assert_eq!(
            parse_depfile("foo.o: foo.c \\\n bar.h\n").unwrap(),
            vec!["foo.c", "bar.h"]
        );
        assert_eq!(parse_depfile("foo.o : foo.c\r\n").unwrap(), vec!["foo.c"]);
        assert_eq!(
            parse_depfile("foo.o: C:/foo.c\n").unwrap(),
            vec!["C:/foo.c"]
        );
        assert_eq!(
            parse_depfile("foo.o:\nbar.o: bar.h\n").unwrap(),
            vec!["bar.h"]
        );
        assert_eq!(parse_depfile("foo.o: foo.c").unwrap(), vec!["foo.c"]);
    }

    #[test]
    fn parse_multiple_targets() {
        assert_eq!(
            parse_depfile("foo.o bar.o: dep.h\n").unwrap(),
            vec!["dep.h"]
        );
    }

    #[test]
    fn parse_multiple_rules() {
        assert_eq!(
            parse_depfile("foo.o: foo.h\n\nbar.o: bar.h\n").unwrap(),
            vec!["foo.h", "bar.h"]
        );
        assert_eq!(
            parse_depfile("foo.o: foo.h bar.o: bar.h\n").unwrap(),
            vec!["foo.h", "bar.o", "bar.h"]
        );
    }

    #[test]
    fn parse_line_continuation() {
        assert_eq!(
            parse_depfile("foo.o: foo.h \\\r\n bar.h\r\n").unwrap(),
            vec!["foo.h", "bar.h"]
        );
        assert_eq!(parse_depfile("foo.o: foo.h \\\n\n").unwrap(), vec!["foo.h"]);
        assert_eq!(
            parse_depfile("foo.o \\\n bar.o: dep.h\n").unwrap(),
            vec!["dep.h"]
        );
    }

    #[test]
    fn parse_escaped_space() {
        assert_eq!(parse_depfile("foo.o: a\\ b.h\n").unwrap(), vec!["a b.h"]);
    }

    #[test]
    fn parse_escaped_colon() {
        assert_eq!(parse_depfile("foo\\:o: a\\:b.h\n").unwrap(), vec!["a:b.h"]);
    }

    #[test]
    fn parse_escaped_hash() {
        assert_eq!(parse_depfile("foo.o: a\\#b.h\n").unwrap(), vec!["a#b.h"]);
    }

    #[test]
    fn parse_backslash_before_other_character() {
        assert_eq!(parse_depfile("foo.o: a\\b.h\n").unwrap(), vec!["a\\b.h"]);
        assert_eq!(parse_depfile("foo.o: a.h\\").unwrap(), vec!["a.h\\"]);
    }

    #[test]
    fn parse_escaped_dollar() {
        assert_eq!(
            parse_depfile("foo.o: a $$b.h\n").unwrap(),
            vec!["a", "$b.h"]
        );
        assert_eq!(parse_depfile("foo.o: a$b.h\n").unwrap(), vec!["a$b.h"]);
    }

    #[test]
    fn parse_comment_line() {
        assert_eq!(
            parse_depfile("foo.o: foo.c\n# comment\n").unwrap(),
            vec!["foo.c"]
        );
        assert_eq!(
            parse_depfile("# leading comment\nfoo.o: foo.c\n").unwrap(),
            vec!["foo.c"]
        );
    }

    #[test]
    fn parse_target_without_space_before_colon() {
        assert!(parse_depfile("foo.o:foo.c\n").is_err());
    }

    #[test]
    fn parse_colon_not_followed_by_whitespace() {
        assert!(parse_depfile("foo.o :foo.c\n").is_err());
    }

    #[test]
    fn parse_missing_colon() {
        assert!(parse_depfile("foo.o foo.c\n").is_err());
        assert!(parse_depfile("foo.o: foo.c\nbar.o\n").is_err());
    }

    #[test]
    fn parse_error_location() {
        assert_eq!(
            parse_depfile("foo.o foo.c\n").unwrap_err(),
            ParseError::new("foo.d:1:12: expected ':'")
        );
        assert_eq!(
            parse_depfile("foo.o: foo.c\nbar.o bar.h\n").unwrap_err(),
            ParseError::new("foo.d:2:12: expected ':'")
        );
        assert_eq!(
            parse_depfile("foo.o").unwrap_err(),
            ParseError::new("foo.d:1:6: expected ':'")
        );
    }
}

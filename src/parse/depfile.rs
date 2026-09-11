use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::tag,
    character::complete::{line_ending, multispace1, none_of, one_of, space1},
    combinator::{all_consuming, eof, map, not, peek, value},
    multi::{many0, many0_count, many1},
    sequence::{preceded, terminated},
};

// cspell: ignore multispace

pub fn depfile(input: &str) -> IResult<&str, Vec<String>> {
    map(
        all_consuming(preceded(blank, many0(terminated(rule, blank)))),
        |rules| rules.into_iter().flatten().collect(),
    )
    .parse(input)
}

fn rule(input: &str) -> IResult<&str, Vec<String>> {
    map(
        (
            not(eof),
            inline_blank,
            many0(terminated(token, inline_blank)),
            colon,
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

    #[test]
    fn parse_empty() {
        assert_eq!(depfile("").unwrap().1, Vec::<String>::new());
        assert_eq!(depfile("\n \t\r\n").unwrap().1, Vec::<String>::new());
    }

    #[test]
    fn parse_dependencies() {
        assert_eq!(
            depfile("foo.o: foo.c foo.h\n").unwrap().1,
            vec!["foo.c", "foo.h"]
        );
        assert_eq!(
            depfile("foo.o: foo.c \\\n bar.h\n").unwrap().1,
            vec!["foo.c", "bar.h"]
        );
        assert_eq!(depfile("foo.o : foo.c\r\n").unwrap().1, vec!["foo.c"]);
        assert_eq!(depfile("foo.o: C:/foo.c\n").unwrap().1, vec!["C:/foo.c"]);
        assert_eq!(depfile("foo.o:\nbar.o: bar.h\n").unwrap().1, vec!["bar.h"]);
        assert_eq!(depfile("foo.o: foo.c").unwrap().1, vec!["foo.c"]);
    }

    #[test]
    fn parse_multiple_targets() {
        assert_eq!(depfile("foo.o bar.o: dep.h\n").unwrap().1, vec!["dep.h"]);
    }

    #[test]
    fn parse_multiple_rules() {
        assert_eq!(
            depfile("foo.o: foo.h\n\nbar.o: bar.h\n").unwrap().1,
            vec!["foo.h", "bar.h"]
        );
        assert_eq!(
            depfile("foo.o: foo.h bar.o: bar.h\n").unwrap().1,
            vec!["foo.h", "bar.o", "bar.h"]
        );
    }

    #[test]
    fn parse_line_continuation() {
        assert_eq!(
            depfile("foo.o: foo.h \\\r\n bar.h\r\n").unwrap().1,
            vec!["foo.h", "bar.h"]
        );
        assert_eq!(depfile("foo.o: foo.h \\\n\n").unwrap().1, vec!["foo.h"]);
        assert_eq!(
            depfile("foo.o \\\n bar.o: dep.h\n").unwrap().1,
            vec!["dep.h"]
        );
    }

    #[test]
    fn parse_escaped_space() {
        assert_eq!(depfile("foo.o: a\\ b.h\n").unwrap().1, vec!["a b.h"]);
    }

    #[test]
    fn parse_escaped_colon() {
        assert_eq!(depfile("foo\\:o: a\\:b.h\n").unwrap().1, vec!["a:b.h"]);
    }

    #[test]
    fn parse_escaped_hash() {
        assert_eq!(depfile("foo.o: a\\#b.h\n").unwrap().1, vec!["a#b.h"]);
    }

    #[test]
    fn parse_backslash_before_other_character() {
        assert_eq!(depfile("foo.o: a\\b.h\n").unwrap().1, vec!["a\\b.h"]);
        assert_eq!(depfile("foo.o: a.h\\").unwrap().1, vec!["a.h\\"]);
    }

    #[test]
    fn parse_escaped_dollar() {
        assert_eq!(depfile("foo.o: a $$b.h\n").unwrap().1, vec!["a", "$b.h"]);
        assert_eq!(depfile("foo.o: a$b.h\n").unwrap().1, vec!["a$b.h"]);
    }

    #[test]
    fn parse_comment_line() {
        assert_eq!(
            depfile("foo.o: foo.c\n# comment\n").unwrap().1,
            vec!["foo.c"]
        );
        assert_eq!(
            depfile("# leading comment\nfoo.o: foo.c\n").unwrap().1,
            vec!["foo.c"]
        );
    }

    #[test]
    fn parse_target_without_space_before_colon() {
        assert!(depfile("foo.o:foo.c\n").is_err());
    }

    #[test]
    fn parse_colon_not_followed_by_whitespace() {
        assert!(depfile("foo.o :foo.c\n").is_err());
    }

    #[test]
    fn parse_missing_colon() {
        assert!(depfile("foo.o foo.c\n").is_err());
        assert!(depfile("foo.o: foo.c\nbar.o\n").is_err());
        assert!(depfile("foo.o").is_err());
    }
}

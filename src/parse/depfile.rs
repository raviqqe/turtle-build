use winnow::{
    Parser, Result,
    ascii::{line_ending, multispace1, space1},
    combinator::{alt, eof, not, peek, preceded, repeat, terminated},
    token::{none_of, one_of, take_till},
};

// cspell: ignore multispace

pub fn depfile(input: &mut &str) -> Result<Vec<String>> {
    preceded(blank, repeat(0.., terminated(rule, blank)))
        .map(|rules: Vec<_>| rules.into_iter().flatten().collect())
        .parse_next(input)
}

fn rule(input: &mut &str) -> Result<Vec<String>> {
    (
        inline_blank,
        repeat(0.., terminated(token, inline_blank)),
        colon,
        terminated(repeat(0.., preceded(inline_blank, token)), inline_blank),
    )
        .map(|(_, (), _, dependencies)| dependencies)
        .parse_next(input)
}

fn token(input: &mut &str) -> Result<String> {
    repeat(1.., token_character).parse_next(input)
}

fn token_character(input: &mut &str) -> Result<char> {
    alt((
        preceded('\\', one_of([' ', '\t', ':', '#'])),
        terminated('\\', not(one_of(['\r', '\n']))),
        "$$".value('$'),
        terminated(':', peek(none_of([' ', '\t', '\r', '\n']))),
        none_of([' ', '\t', '\r', '\n', '\\', ':']),
    ))
    .parse_next(input)
}

fn colon(input: &mut &str) -> Result<()> {
    terminated(":", peek(alt((multispace1, eof))))
        .void()
        .parse_next(input)
}

fn inline_blank(input: &mut &str) -> Result<()> {
    repeat(0.., alt((space1.void(), ("\\", newline).void()))).parse_next(input)
}

fn newline<'a>(input: &mut &'a str) -> Result<&'a str> {
    alt((line_ending, "\r")).parse_next(input)
}

fn blank(input: &mut &str) -> Result<()> {
    repeat(0.., alt((multispace1.void(), comment))).parse_next(input)
}

fn comment(input: &mut &str) -> Result<()> {
    ("#", take_till(0.., '\n')).void().parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parse_empty() {
        assert_eq!(depfile.parse("").unwrap(), Vec::<String>::new());
        assert_eq!(depfile.parse("\n \t\r\n").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn parse_dependencies() {
        assert_eq!(
            depfile.parse("foo.o: foo.c foo.h\n").unwrap(),
            vec!["foo.c", "foo.h"]
        );
        assert_eq!(
            depfile.parse("foo.o: foo.c \\\n bar.h\n").unwrap(),
            vec!["foo.c", "bar.h"]
        );
        assert_eq!(depfile.parse("foo.o : foo.c\r\n").unwrap(), vec!["foo.c"]);
        assert_eq!(
            depfile.parse("foo.o: C:/foo.c\n").unwrap(),
            vec!["C:/foo.c"]
        );
        assert_eq!(
            depfile.parse("foo.o:\nbar.o: bar.h\n").unwrap(),
            vec!["bar.h"]
        );
        assert_eq!(depfile.parse("foo.o: foo.c").unwrap(), vec!["foo.c"]);
    }

    #[test]
    fn parse_multiple_targets() {
        assert_eq!(
            depfile.parse("foo.o bar.o: dep.h\n").unwrap(),
            vec!["dep.h"]
        );
    }

    #[test]
    fn parse_multiple_rules() {
        assert_eq!(
            depfile.parse("foo.o: foo.h\n\nbar.o: bar.h\n").unwrap(),
            vec!["foo.h", "bar.h"]
        );
        assert_eq!(
            depfile.parse("foo.o: foo.h bar.o: bar.h\n").unwrap(),
            vec!["foo.h", "bar.o", "bar.h"]
        );
        assert_eq!(
            depfile.parse("foo.o: foo.h : bar.h\n").unwrap(),
            vec!["foo.h", "bar.h"]
        );
    }

    #[test]
    fn parse_line_continuation() {
        assert_eq!(
            depfile.parse("foo.o: foo.h \\\r\n bar.h\r\n").unwrap(),
            vec!["foo.h", "bar.h"]
        );
        assert_eq!(
            depfile.parse("foo.o: foo.h \\\r bar.h\r").unwrap(),
            vec!["foo.h", "bar.h"]
        );
        assert_eq!(depfile.parse("foo.o: foo.h \\\n\n").unwrap(), vec!["foo.h"]);
        assert_eq!(
            depfile.parse("foo.o \\\n bar.o: dep.h\n").unwrap(),
            vec!["dep.h"]
        );
    }

    #[test]
    fn parse_escaped_space() {
        assert_eq!(depfile.parse("foo.o: a\\ b.h\n").unwrap(), vec!["a b.h"]);
    }

    #[test]
    fn parse_escaped_colon() {
        assert_eq!(depfile.parse("foo\\:o: a\\:b.h\n").unwrap(), vec!["a:b.h"]);
    }

    #[test]
    fn parse_escaped_hash() {
        assert_eq!(depfile.parse("foo.o: a\\#b.h\n").unwrap(), vec!["a#b.h"]);
    }

    #[test]
    fn parse_backslash_before_other_character() {
        assert_eq!(depfile.parse("foo.o: a\\b.h\n").unwrap(), vec!["a\\b.h"]);
        assert_eq!(depfile.parse("foo.o: a.h\\").unwrap(), vec!["a.h\\"]);
    }

    #[test]
    fn parse_escaped_dollar() {
        assert_eq!(
            depfile.parse("foo.o: a $$b.h\n").unwrap(),
            vec!["a", "$b.h"]
        );
        assert_eq!(depfile.parse("foo.o: a$b.h\n").unwrap(), vec!["a$b.h"]);
    }

    #[test]
    fn parse_comment_line() {
        assert_eq!(
            depfile.parse("foo.o: foo.c\n# comment\n").unwrap(),
            vec!["foo.c"]
        );
        assert_eq!(
            depfile.parse("# leading comment\nfoo.o: foo.c\n").unwrap(),
            vec!["foo.c"]
        );
    }

    #[test]
    fn parse_target_without_space_before_colon() {
        assert!(depfile.parse("foo.o:foo.c\n").is_err());
    }

    #[test]
    fn parse_colon_not_followed_by_whitespace() {
        assert!(depfile.parse("foo.o :foo.c\n").is_err());
    }

    #[test]
    fn parse_missing_colon() {
        assert!(depfile.parse("foo.o foo.c\n").is_err());
        assert!(depfile.parse("foo.o: foo.c\nbar.o\n").is_err());
        assert!(depfile.parse("foo.o").is_err());
    }
}

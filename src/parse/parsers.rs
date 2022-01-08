use super::stream::Stream;
use crate::cfg::{Build, Module, Rule, VariableDefinition};
use combine::{
    attempt, choice, eof, many, many1, none_of, not_followed_by, one_of, optional,
    parser::char::{alpha_num, char, letter, newline, string},
    value, Parser,
};

pub fn module<'a>() -> impl Parser<Stream<'a>, Output = Module> {
    (
        optional(line_break()),
        many(variable_definition()),
        many(rule()),
        many(build()),
    )
        .skip(eof())
        .map(|(_, variable_definitions, rules, builds)| {
            Module::new(variable_definitions, rules, builds, vec![])
        })
}

fn variable_definition<'a>() -> impl Parser<Stream<'a>, Output = VariableDefinition> {
    (attempt(identifier().skip(sign("="))), string_line())
        .skip(line_break())
        .map(|(name, value)| VariableDefinition::new(name, value))
}

fn rule<'a>() -> impl Parser<Stream<'a>, Output = Rule> {
    (
        keyword("rule"),
        identifier(),
        line_break(),
        (string("\t"), keyword("command"), sign("="))
            .with(string_line())
            .skip(line_break()),
        optional(
            (string("\t"), keyword("description"), sign("="))
                .with(string_line())
                .skip(line_break()),
        ),
    )
        .map(|(_, name, _, command, description)| {
            Rule::new(name, command, description.unwrap_or_default())
        })
}

fn build<'a>() -> impl Parser<Stream<'a>, Output = Build> {
    (
        keyword("build"),
        many1(string_literal()),
        sign(":"),
        identifier(),
        many1(string_literal()),
    )
        .skip(line_break())
        .map(|(_, outputs, _, rule, inputs)| Build::new(outputs, rule, inputs))
}

fn string_line<'a>() -> impl Parser<Stream<'a>, Output = String> {
    many1(none_of(['\n'])).map(|string: String| string.trim().into())
}

fn string_literal<'a>() -> impl Parser<Stream<'a>, Output = String> {
    many1(none_of([' ', '\t', '\r', '\n', ':']))
}

fn keyword<'a>(name: &'static str) -> impl Parser<Stream<'a>, Output = ()> {
    token(attempt(string(name)).skip(not_followed_by(alpha_num()))).with(value(()))
}

fn identifier<'a>() -> impl Parser<Stream<'a>, Output = String> {
    token(
        (
            choice((letter(), char('_'))),
            many(choice((alpha_num(), char('_')))),
        )
            .map(|(head, tail): (_, String)| [head.into(), tail].concat()),
    )
}

fn sign<'a>(sign: &'static str) -> impl Parser<Stream<'a>, Output = ()> {
    token(string(sign)).with(value(()))
}

fn token<'a, O, P: Parser<Stream<'a>, Output = O>>(
    parser: P,
) -> impl Parser<Stream<'a>, Output = O> {
    parser.skip(blank())
}

fn blank<'a>() -> impl Parser<Stream<'a>, Output = ()> {
    many::<Vec<_>, _, _>(one_of([' ', '\t', '\r'])).with(value(()))
}

fn line_break<'a>() -> impl Parser<Stream<'a>, Output = ()> {
    many1::<Vec<_>, _, _>(attempt((blank(), newline()))).with(value(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::stream::stream;

    #[test]
    fn parse_module() {
        assert_eq!(
            module().parse(stream("")).unwrap().0,
            Module::new(vec![], vec![], vec![], vec![])
        );
        assert_eq!(
            module().parse(stream("x = 42\n")).unwrap().0,
            Module::new(
                vec![VariableDefinition::new("x", "42")],
                vec![],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            module().parse(stream("x = 1\ny = 2\n")).unwrap().0,
            Module::new(
                vec![
                    VariableDefinition::new("x", "1"),
                    VariableDefinition::new("y", "2"),
                ],
                vec![],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            module()
                .parse(stream("rule foo\n\tcommand = bar\n"))
                .unwrap()
                .0,
            Module::new(vec![], vec![Rule::new("foo", "bar", "")], vec![], vec![])
        );
        assert_eq!(
            module()
                .parse(stream(
                    "rule foo\n\tcommand = bar\nrule baz\n\tcommand = blah\n"
                ))
                .unwrap()
                .0,
            Module::new(
                vec![],
                vec![Rule::new("foo", "bar", ""), Rule::new("baz", "blah", "")],
                vec![],
                vec![]
            )
        );
    }

    #[test]
    fn parse_variable_definition() {
        assert_eq!(
            variable_definition().parse(stream("x = 42\n")).unwrap().0,
            VariableDefinition::new("x", "42")
        );
        assert_eq!(
            variable_definition()
                .parse(stream("foo = 1 + 1\n"))
                .unwrap()
                .0,
            VariableDefinition::new("foo", "1 + 1")
        );
    }

    #[test]
    fn parse_rule() {
        assert_eq!(
            rule()
                .parse(stream("rule foo\n\tcommand = bar\n"))
                .unwrap()
                .0,
            Rule::new("foo", "bar", "")
        );
        assert_eq!(
            rule()
                .parse(stream("rule foo\n\tcommand = bar\n\tdescription = baz\n"))
                .unwrap()
                .0,
            Rule::new("foo", "bar", "baz")
        );
    }

    #[test]
    fn parse_build() {
        assert_eq!(
            build().parse(stream("build foo: bar baz\n")).unwrap().0,
            Build::new(vec!["foo".into()], "bar", vec!["baz".into()])
        );
    }

    #[test]
    fn parse_string_line() {
        assert!(string_line().parse(stream("")).is_err());
        assert_eq!(
            string_line().parse(stream("foo")).unwrap().0,
            "foo".to_string()
        );
        assert_eq!(
            string_line().parse(stream("foo\n")).unwrap().0,
            "foo".to_string()
        );
        assert_eq!(
            string_line().parse(stream("foo \n")).unwrap().0,
            "foo".to_string()
        );
        assert_eq!(
            string_line().parse(stream("foo bar")).unwrap().0,
            "foo bar".to_string()
        );
    }

    #[test]
    fn parse_string_literal() {
        assert!(string_literal().parse(stream("")).is_err());
        assert_eq!(
            string_literal().parse(stream("foo")).unwrap().0,
            "foo".to_string()
        );
        assert_eq!(
            string_literal().parse(stream("foo bar")).unwrap().0,
            "foo".to_string()
        );
    }

    #[test]
    fn parse_identifier() {
        assert_eq!(
            identifier().parse(stream("foo")).unwrap().0,
            "foo".to_string()
        );
        assert_eq!(
            identifier().parse(stream("foo bar")).unwrap().0,
            "foo".to_string()
        );
        assert_eq!(
            identifier().parse(stream("foo_bar")).unwrap().0,
            "foo_bar".to_string()
        );
        assert_eq!(
            identifier().parse(stream("_foo")).unwrap().0,
            "_foo".to_string()
        );
    }

    #[test]
    fn parse_blank() {
        assert!(blank().skip(eof()).parse(stream("")).is_ok());
        assert!(blank().skip(eof()).parse(stream(" ")).is_ok());
        assert!(blank().skip(eof()).parse(stream("\t")).is_ok());
        assert!(blank().skip(eof()).parse(stream("\r")).is_ok());
        assert!(blank().skip(eof()).parse(stream("  ")).is_ok());
        assert!(blank().skip(eof()).parse(stream(" \t")).is_ok());
        assert!(blank().skip(eof()).parse(stream("\n")).is_err());
        assert!(blank().skip(eof()).parse(stream(" \n")).is_err());
    }

    #[test]
    fn parse_line_break() {
        assert!(line_break().skip(eof()).parse(stream("")).is_err());
        assert!(line_break().skip(eof()).parse(stream("\n")).is_ok());
        assert!(line_break().skip(eof()).parse(stream(" \n")).is_ok());
        assert!(line_break().skip(eof()).parse(stream("  \n")).is_ok());
        assert!(line_break().skip(eof()).parse(stream("\n\n")).is_ok());
        assert!(line_break().skip(eof()).parse(stream("\n ")).is_err());
    }
}

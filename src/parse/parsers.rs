use super::stream::Stream;
use crate::cfg::{Module, VariableDefinition};
use combine::{
    choice, eof, many, many1, none_of, one_of, optional,
    parser::char::{alpha_num, char, letter, newline, string},
    value, Parser,
};

pub fn module<'a>() -> impl Parser<Stream<'a>, Output = Module> {
    (
        optional(line_break()),
        many(variable_definition().skip(line_break())),
    )
        .skip(eof())
        .map(|(_, variable_definitions)| Module::new(variable_definitions, vec![], vec![], vec![]))
}

fn variable_definition<'a>() -> impl Parser<Stream<'a>, Output = VariableDefinition> {
    (identifier(), sign("="), string_literal())
        .map(|(name, _, value)| VariableDefinition::new(name, value))
}

fn string_literal<'a>() -> impl Parser<Stream<'a>, Output = String> {
    many(none_of(['\n'])).map(|string: String| string.trim().into())
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
    many1::<Vec<_>, _, _>((blank(), newline())).with(value(()))
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
    }

    #[test]
    fn parse_variable_definition() {
        assert_eq!(
            variable_definition().parse(stream("x = 42")).unwrap().0,
            VariableDefinition::new("x", "42")
        );
        assert_eq!(
            variable_definition()
                .parse(stream("foo = 1 + 1"))
                .unwrap()
                .0,
            VariableDefinition::new("foo", "1 + 1")
        );
    }

    #[test]
    fn parse_string_literal() {
        assert_eq!(
            string_literal().parse(stream("foo")).unwrap().0,
            "foo".to_string()
        );
        assert_eq!(
            string_literal().parse(stream("foo\n")).unwrap().0,
            "foo".to_string()
        );
        assert_eq!(
            string_literal().parse(stream("foo \n")).unwrap().0,
            "foo".to_string()
        );
        assert_eq!(
            string_literal().parse(stream("foo bar")).unwrap().0,
            "foo bar".to_string()
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

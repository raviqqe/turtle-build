use super::stream::Stream;
use crate::cfg::{Module, VariableDefinition};
use combine::{
    choice, eof, many, none_of,
    parser::char::{alpha_num, char, letter, newline, space, string},
    value, Parser,
};

pub fn module<'a>() -> impl Parser<Stream<'a>, Output = Module> {
    (blank(), many(variable_definition()))
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
            .map(|(head, tail): (char, String)| [head.into(), tail].concat()),
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
    many::<Vec<_>, _, _>(space()).with(value(()))
}

fn blank_lines<'a>() -> impl Parser<Stream<'a>, Output = ()> {
    many::<Vec<_>, _, _>(newline()).with(value(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::stream::stream;

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
}

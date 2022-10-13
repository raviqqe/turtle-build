use super::stream::Stream;
use crate::ast::{
    Build, DefaultOutput, DynamicBuild, DynamicModule, Include, Module, Rule, Statement, Submodule,
    VariableDefinition,
};
use combine::{
    attempt, choice, eof, many, many1, none_of, not_followed_by, one_of, optional,
    parser::char::{alpha_num, char, letter, newline, string},
    unexpected_any, value, Parser,
};

const OPERATOR_CHARACTERS: &[char] = &['|', ':'];
const DYNAMIC_MODULE_VERSION_VARIABLE: &str = "ninja_dyndep_version";

pub fn module<'a>() -> impl Parser<Stream<'a>, Output = Module> {
    (optional(line_break()), many(statement()))
        .skip(eof())
        .map(|(_, statements)| Module::new(statements))
}

pub fn dynamic_module<'a>() -> impl Parser<Stream<'a>, Output = DynamicModule> {
    (
        optional(line_break()),
        dynamic_module_version(),
        many(dynamic_build()),
    )
        .skip(eof())
        .map(|(_, _, builds)| DynamicModule::new(builds))
}

fn statement<'a>() -> impl Parser<Stream<'a>, Output = Statement> {
    choice((
        build().map(Statement::from),
        default().map(Statement::from),
        include().map(Statement::from),
        rule().map(Statement::from),
        submodule().map(Statement::from),
        variable_definition().map(Statement::from),
    ))
}

fn variable_definition<'a>() -> impl Parser<Stream<'a>, Output = VariableDefinition> {
    (
        attempt(identifier().skip(sign("="))),
        optional(string_line()),
    )
        .skip(line_break())
        .map(|(name, value)| VariableDefinition::new(name, value.unwrap_or_default()))
}

fn dynamic_module_version<'a>() -> impl Parser<Stream<'a>, Output = String> {
    attempt((keyword(DYNAMIC_MODULE_VERSION_VARIABLE), sign("=")))
        .with(string_line())
        .skip(line_break())
}

fn rule<'a>() -> impl Parser<Stream<'a>, Output = Rule> {
    (
        keyword("rule"),
        identifier(),
        line_break(),
        many(indent().with(variable_definition())),
    )
        .then(|(_, name, _, variable_definitions): (_, _, _, Vec<_>)| {
            if let Some(command) = variable_definitions.iter().find_map(|definition| {
                if definition.name() == "command" {
                    Some(definition.value())
                } else {
                    None
                }
            }) {
                value(Rule::new(
                    name,
                    command,
                    variable_definitions.iter().find_map(|definition| {
                        if definition.name() == "description" {
                            Some(definition.value().into())
                        } else {
                            None
                        }
                    }),
                    variable_definitions
                        .iter()
                        .any(|definition| definition.name() == "always"),
                ))
                .right()
            } else {
                unexpected_any("missing command variable").left()
            }
        })
        .expected("rule statement")
}

fn build<'a>() -> impl Parser<Stream<'a>, Output = Build> {
    (
        keyword("build"),
        many1(string_literal()),
        optional(sign("|").with(many1::<Vec<_>, _, _>(string_literal()))),
        sign(":"),
        identifier(),
        many(string_literal()),
        optional(sign("|").with(many1::<Vec<_>, _, _>(string_literal()))),
        optional(sign("||").with(many1::<Vec<_>, _, _>(string_literal()))),
        line_break(),
        many(indent().with(variable_definition())),
    )
        .map(
            |(
                _,
                outputs,
                implicit_outputs,
                _,
                rule,
                inputs,
                implicit_inputs,
                order_only_inputs,
                _,
                variable_definitions,
            )| {
                Build::new(
                    outputs,
                    implicit_outputs.into_iter().flatten().collect(),
                    rule,
                    inputs,
                    implicit_inputs.into_iter().flatten().collect(),
                    order_only_inputs.into_iter().flatten().collect(),
                    variable_definitions,
                )
            },
        )
        .expected("build statement")
}

pub fn dynamic_build<'a>() -> impl Parser<Stream<'a>, Output = DynamicBuild> {
    (
        keyword("build"),
        string_literal(),
        sign(":"),
        keyword("dyndep"),
        optional(sign("|").with(many1::<Vec<_>, _, _>(string_literal()))),
        line_break(),
    )
        .map(|(_, output, _, _, implicit_inputs, _)| {
            DynamicBuild::new(output, implicit_inputs.into_iter().flatten().collect())
        })
        .expected("build statement")
}

fn default<'a>() -> impl Parser<Stream<'a>, Output = DefaultOutput> {
    (keyword("default"), many1(string_literal()))
        .skip(line_break())
        .map(|(_, outputs)| DefaultOutput::new(outputs))
        .expected("default statement")
}

fn include<'a>() -> impl Parser<Stream<'a>, Output = Include> {
    (keyword("include"), string_line())
        .skip(line_break())
        .map(|(_, path)| Include::new(path))
        .expected("include statement")
}

fn submodule<'a>() -> impl Parser<Stream<'a>, Output = Submodule> {
    (keyword("subninja"), string_line())
        .skip(line_break())
        .map(|(_, path)| Submodule::new(path))
        .expected("subninja statement")
}

fn string_line<'a>() -> impl Parser<Stream<'a>, Output = String> {
    many1(none_of(['\n'])).map(|string: String| string.trim().into())
}

fn string_literal<'a>() -> impl Parser<Stream<'a>, Output = String> {
    token(many1(none_of(
        [' ', '\t', '\r', '\n']
            .into_iter()
            .chain(OPERATOR_CHARACTERS.iter().cloned()),
    )))
}

fn keyword<'a>(name: &'static str) -> impl Parser<Stream<'a>, Output = ()> {
    token(attempt(string(name).skip(not_followed_by(alpha_num()))))
        .with(value(()))
        .expected(name)
}

fn identifier<'a>() -> impl Parser<Stream<'a>, Output = String> {
    token(
        (
            choice((letter(), char('_'))),
            many(choice((alpha_num(), char('_')))),
        )
            .map(|(head, tail): (_, String)| [head.into(), tail].concat()),
    )
    .expected("identifier")
}

fn sign<'a>(sign: &'static str) -> impl Parser<Stream<'a>, Output = ()> {
    attempt(token(string(sign)).skip(not_followed_by(one_of(OPERATOR_CHARACTERS.iter().cloned()))))
        .with(value(()))
        .expected(sign)
}

fn token<'a, O, P: Parser<Stream<'a>, Output = O>>(
    parser: P,
) -> impl Parser<Stream<'a>, Output = O> {
    parser.skip(blank())
}

fn indent<'a>() -> impl Parser<Stream<'a>, Output = ()> {
    many1::<Vec<_>, _, _>(char(' '))
        .with(value(()))
        .expected("indent")
}

fn blank<'a>() -> impl Parser<Stream<'a>, Output = ()> {
    many::<Vec<_>, _, _>(choice((space(), comment()))).with(value(()))
}

fn space<'a>() -> impl Parser<Stream<'a>, Output = ()> {
    one_of([' ', '\t', '\r']).with(value(())).expected("space")
}

fn comment<'a>() -> impl Parser<Stream<'a>, Output = ()> {
    (string("#"), many::<Vec<_>, _, _>(none_of("\n".chars())))
        .with(value(()))
        .expected("comment")
}

fn line_break<'a>() -> impl Parser<Stream<'a>, Output = ()> {
    many1::<Vec<_>, _, _>(attempt((blank(), newline()))).with(value(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::stream::stream;

    fn explicit_build(
        outputs: Vec<String>,
        rule: impl Into<String>,
        inputs: Vec<String>,
        variable_definitions: Vec<VariableDefinition>,
    ) -> Build {
        Build::new(
            outputs,
            vec![],
            rule,
            inputs,
            vec![],
            vec![],
            variable_definitions,
        )
    }

    #[test]
    fn parse_module() {
        assert_eq!(module().parse(stream("")).unwrap().0, Module::new(vec![]));
        assert_eq!(
            module().parse(stream("#foo\n")).unwrap().0,
            Module::new(vec![])
        );
        assert_eq!(
            module().parse(stream("x = 42\n")).unwrap().0,
            Module::new(vec![VariableDefinition::new("x", "42").into()])
        );
        assert_eq!(
            module().parse(stream("x = 1\ny = 2\n")).unwrap().0,
            Module::new(vec![
                VariableDefinition::new("x", "1").into(),
                VariableDefinition::new("y", "2").into(),
            ],)
        );
        assert_eq!(
            module()
                .parse(stream("rule foo\n command = bar\n"))
                .unwrap()
                .0,
            Module::new(vec![Rule::new("foo", "bar", None, false).into()])
        );
        assert_eq!(
            module()
                .parse(stream(
                    "rule foo\n command = bar\nrule baz\n command = blah\n"
                ))
                .unwrap()
                .0,
            Module::new(vec![
                Rule::new("foo", "bar", None, false).into(),
                Rule::new("baz", "blah", None, false).into(),
            ],)
        );
        assert_eq!(
            module().parse(stream("builddir = foo\n")).unwrap().0,
            Module::new(vec![VariableDefinition::new("builddir", "foo").into()])
        );
    }

    #[test]
    fn parse_dynamic_module() {
        assert_eq!(
            dynamic_module()
                .parse(stream("ninja_dyndep_version = 1\n"))
                .unwrap()
                .0,
            DynamicModule::new(vec![])
        );
        assert_eq!(
            dynamic_module()
                .parse(stream("ninja_dyndep_version = 1\nbuild foo: dyndep\n"))
                .unwrap()
                .0,
            DynamicModule::new(vec![DynamicBuild::new("foo", vec![])])
        );
        assert_eq!(
            dynamic_module()
                .parse(stream(
                    "ninja_dyndep_version = 1\nbuild foo: dyndep\nbuild bar: dyndep\n"
                ))
                .unwrap()
                .0,
            DynamicModule::new(vec![
                DynamicBuild::new("foo", vec![]),
                DynamicBuild::new("bar", vec![])
            ])
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
        assert_eq!(
            variable_definition().parse(stream("x =\n")).unwrap().0,
            VariableDefinition::new("x", "")
        );
        assert_eq!(
            variable_definition().parse(stream("x = \n")).unwrap().0,
            VariableDefinition::new("x", "")
        );
    }

    #[test]
    fn parse_dynamic_module_version() {
        assert_eq!(
            dynamic_module_version()
                .parse(stream("ninja_dyndep_version = 42\n"))
                .unwrap()
                .0,
            "42".to_string(),
        );
    }

    #[test]
    fn parse_rule() {
        assert_eq!(
            rule()
                .parse(stream("rule foo\n command = bar\n"))
                .unwrap()
                .0,
            Rule::new("foo", "bar", None, false)
        );
        assert_eq!(
            rule()
                .parse(stream("rule foo\n command = bar\n description = baz\n"))
                .unwrap()
                .0,
            Rule::new("foo", "bar", Some("baz".into()), false)
        );
        assert_eq!(
            rule()
                .parse(stream("rule foo\n command = bar\n always = true\n"))
                .unwrap()
                .0,
            Rule::new("foo", "bar", None, true)
        );
    }

    #[test]
    fn parse_build() {
        assert_eq!(
            build().parse(stream("build foo: bar\n")).unwrap().0,
            explicit_build(vec!["foo".into()], "bar", vec![], vec![])
        );
        assert_eq!(
            build().parse(stream("build foo: bar baz\n")).unwrap().0,
            explicit_build(vec!["foo".into()], "bar", vec!["baz".into()], vec![])
        );
        assert_eq!(
            build()
                .parse(stream("build foo: bar baz blah\n"))
                .unwrap()
                .0,
            explicit_build(
                vec!["foo".into()],
                "bar",
                vec!["baz".into(), "blah".into()],
                vec![]
            )
        );
        assert_eq!(
            build().parse(stream("build foo bar: baz\n")).unwrap().0,
            explicit_build(vec!["foo".into(), "bar".into()], "baz", vec![], vec![])
        );
        assert_eq!(
            build().parse(stream("build foo: bar\n x = 1\n")).unwrap().0,
            explicit_build(
                vec!["foo".into()],
                "bar",
                vec![],
                vec![VariableDefinition::new("x", "1")]
            )
        );
        assert_eq!(
            build()
                .parse(stream("build foo: bar\n x = 1\n y = 2\n"))
                .unwrap()
                .0,
            explicit_build(
                vec!["foo".into()],
                "bar",
                vec![],
                vec![
                    VariableDefinition::new("x", "1"),
                    VariableDefinition::new("y", "2")
                ]
            )
        );
        assert_eq!(
            build().parse(stream("build x1 | x2: rule\n")).unwrap().0,
            Build::new(
                vec!["x1".into()],
                vec!["x2".into()],
                "rule",
                vec![],
                vec![],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build().parse(stream("build x1 | x2 x3: rule\n")).unwrap().0,
            Build::new(
                vec!["x1".into()],
                vec!["x2".into(), "x3".into()],
                "rule",
                vec![],
                vec![],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build().parse(stream("build x1: rule | x2\n")).unwrap().0,
            Build::new(
                vec!["x1".into()],
                vec![],
                "rule",
                vec![],
                vec!["x2".into()],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build().parse(stream("build x1: rule | x2 x3\n")).unwrap().0,
            Build::new(
                vec!["x1".into()],
                vec![],
                "rule",
                vec![],
                vec!["x2".into(), "x3".into()],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build().parse(stream("build x1: rule || x2\n")).unwrap().0,
            Build::new(
                vec!["x1".into()],
                vec![],
                "rule",
                vec![],
                vec![],
                vec!["x2".into()],
                vec![],
            )
        );
        assert_eq!(
            build()
                .parse(stream("build x1: rule || x2 x3\n"))
                .unwrap()
                .0,
            Build::new(
                vec!["x1".into()],
                vec![],
                "rule",
                vec![],
                vec![],
                vec!["x2".into(), "x3".into()],
                vec![]
            )
        );
    }

    #[test]
    fn parse_dynamic_build() {
        assert_eq!(
            dynamic_build()
                .parse(stream("build foo: dyndep\n"))
                .unwrap()
                .0,
            DynamicBuild::new("foo", vec![])
        );
        assert_eq!(
            dynamic_build()
                .parse(stream("build foo: dyndep | bar\n"))
                .unwrap()
                .0,
            DynamicBuild::new("foo", vec!["bar".into()])
        );
        assert_eq!(
            dynamic_build()
                .parse(stream("build foo: dyndep | bar baz\n"))
                .unwrap()
                .0,
            DynamicBuild::new("foo", vec!["bar".into(), "baz".into()])
        );
    }

    #[test]
    fn parse_default() {
        assert!(default().parse(stream("")).is_err());
        assert!(default().parse(stream("default\n")).is_err());
        assert_eq!(
            default().parse(stream("default foo\n")).unwrap().0,
            DefaultOutput::new(vec!["foo".into()])
        );
        assert_eq!(
            default().parse(stream("default foo bar\n")).unwrap().0,
            DefaultOutput::new(vec!["foo".into(), "bar".into()])
        );
    }

    #[test]
    fn parse_include() {
        assert_eq!(
            include().parse(stream("include foo\n")).unwrap().0,
            Include::new("foo")
        );
    }

    #[test]
    fn parse_submodule() {
        assert_eq!(
            submodule().parse(stream("subninja foo\n")).unwrap().0,
            Submodule::new("foo")
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
    fn parse_keyword() {
        assert!(keyword("foo").parse(stream("foo")).is_ok());
        assert!(keyword("fo").parse(stream("foo")).is_err());
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
        assert!(blank().skip(eof()).parse(stream("#")).is_ok());
        assert!(blank().skip(eof()).parse(stream("#foo")).is_ok());
        assert!(blank().skip(eof()).parse(stream(" #foo")).is_ok());
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

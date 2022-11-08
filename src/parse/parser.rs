use crate::ast::{
    Build, DefaultOutput, DynamicBuild, DynamicModule, Include, Module, Rule, Statement, Submodule,
    VariableDefinition,
};
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, alphanumeric1, newline, none_of, one_of, space1},
    combinator::{all_consuming, into, map, not, opt, peek, recognize, value},
    multi::{many0, many0_count, many1, many1_count},
    sequence::{delimited, preceded, terminated, tuple},
    IResult, Parser,
};

const OPERATOR_CHARACTERS: &str = "|:";
const DYNAMIC_MODULE_VERSION_VARIABLE: &str = "ninja_dyndep_version";

pub fn module(input: &str) -> IResult<&str, Module> {
    map(
        all_consuming(tuple((opt(line_break), many0(statement), opt(line_break)))),
        |(_, statements, _)| Module::new(statements),
    )(input)
}

pub fn dynamic_module(input: &str) -> IResult<&str, DynamicModule> {
    map(
        all_consuming(tuple((
            opt(line_break),
            dynamic_module_version,
            many0(dynamic_build),
        ))),
        |(_, _, builds)| DynamicModule::new(builds),
    )(input)
}

fn statement(input: &str) -> IResult<&str, Statement> {
    alt((
        into(build),
        into(default),
        into(include),
        into(rule),
        into(submodule),
        into(variable_definition),
    ))(input)
}

fn variable_definition(input: &str) -> IResult<&str, VariableDefinition> {
    map(
        tuple((identifier, sign("="), opt(string_line), line_break)),
        |(name, _, value, _)| VariableDefinition::new(name, value.unwrap_or_default()),
    )(input)
}

fn dynamic_module_version(input: &str) -> IResult<&str, &str> {
    map(
        tuple((
            keyword(DYNAMIC_MODULE_VERSION_VARIABLE),
            sign("="),
            string_line,
            line_break,
        )),
        |(_, _, version, _)| version,
    )(input)
}

fn rule(input: &str) -> IResult<&str, Rule> {
    map(
        tuple((
            keyword("rule"),
            identifier,
            line_break,
            delimited(
                tuple((indent, keyword("command"), sign("="))),
                string_line,
                line_break,
            ),
            opt(delimited(
                tuple((indent, keyword("description"), sign("="))),
                string_line,
                line_break,
            )),
        )),
        |(_, name, _, command, description)| Rule::new(name, command, description.map(From::from)),
    )(input)
}

fn build(input: &str) -> IResult<&str, Build> {
    map(
        tuple((
            keyword("build"),
            many1(string_literal),
            opt(preceded(sign("|"), many1(string_literal))),
            sign(":"),
            identifier,
            many0(string_literal),
            opt(preceded(sign("|"), many1(string_literal))),
            opt(preceded(sign("||"), many1(string_literal))),
            line_break,
            many0(preceded(indent, variable_definition)),
        )),
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
                outputs.into_iter().map(From::from).collect(),
                implicit_outputs
                    .into_iter()
                    .flatten()
                    .map(From::from)
                    .collect(),
                rule,
                inputs.into_iter().map(From::from).collect(),
                implicit_inputs
                    .into_iter()
                    .flatten()
                    .map(From::from)
                    .collect(),
                order_only_inputs
                    .into_iter()
                    .flatten()
                    .map(From::from)
                    .collect(),
                variable_definitions,
            )
        },
    )(input)
}

pub fn dynamic_build(input: &str) -> IResult<&str, DynamicBuild> {
    map(
        tuple((
            keyword("build"),
            string_literal,
            sign(":"),
            keyword("dyndep"),
            opt(preceded(sign("|"), many1(string_literal))),
            line_break,
        )),
        |(_, output, _, _, implicit_inputs, _)| {
            DynamicBuild::new(
                output,
                implicit_inputs
                    .into_iter()
                    .flatten()
                    .map(From::from)
                    .collect(),
            )
        },
    )(input)
}

fn default(input: &str) -> IResult<&str, DefaultOutput> {
    map(
        tuple((keyword("default"), many1(string_literal), line_break)),
        |(_, outputs, _)| DefaultOutput::new(outputs.into_iter().map(From::from).collect()),
    )(input)
}

fn include(input: &str) -> IResult<&str, Include> {
    map(
        tuple((keyword("include"), string_line, line_break)),
        |(_, path, _)| Include::new(path),
    )(input)
}

fn submodule(input: &str) -> IResult<&str, Submodule> {
    map(
        tuple((keyword("subninja"), string_line, line_break)),
        |(_, path, _)| Submodule::new(path),
    )(input)
}

fn string_line(input: &str) -> IResult<&str, &str> {
    map(recognize(many1_count(none_of("\n"))), |string: &str| {
        string.trim()
    })(input)
}

fn string_literal(input: &str) -> IResult<&str, &str> {
    token(recognize(many1_count(none_of(
        &*(" \t\r\n".to_owned() + OPERATOR_CHARACTERS),
    ))))(input)
}

fn keyword(name: &'static str) -> impl Fn(&str) -> IResult<&str, ()> {
    move |input| value((), token(tuple((tag(name), peek(not(alphanumeric1))))))(input)
}

fn identifier(input: &str) -> IResult<&str, &str> {
    token(recognize(tuple((
        alt((alpha1, tag("_"))),
        many0_count(alt((alphanumeric1, tag("_")))),
    ))))(input)
}

fn sign(sign: &'static str) -> impl Fn(&str) -> IResult<&str, ()> {
    move |input| {
        value(
            (),
            token(terminated(
                tag(sign),
                peek(not(one_of(OPERATOR_CHARACTERS))),
            )),
        )(input)
    }
}

fn token<'a, O>(
    mut parser: impl Parser<&'a str, O, nom::error::Error<&'a str>>,
) -> impl FnMut(&'a str) -> IResult<&'a str, O> {
    move |input| {
        let (input, _) = blank(input)?;

        parser.parse(input)
    }
}

fn indent(input: &str) -> IResult<&str, ()> {
    value((), space1)(input)
}

fn blank(input: &str) -> IResult<&str, ()> {
    value((), many0_count(alt((value((), space1), comment))))(input)
}

fn comment(input: &str) -> IResult<&str, ()> {
    value((), tuple((tag("#"), many0_count(none_of("\n")))))(input)
}

fn line_break(input: &str) -> IResult<&str, ()> {
    value((), many1_count(tuple((blank, newline))))(input)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Module::new(vec![Rule::new("foo", "bar", None).into()])
        );
        assert_eq!(
            module()
                .parse(stream(
                    "rule foo\n command = bar\nrule baz\n command = blah\n"
                ))
                .unwrap()
                .0,
            Module::new(vec![
                Rule::new("foo", "bar", None).into(),
                Rule::new("baz", "blah", None).into(),
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
            Rule::new("foo", "bar", None)
        );
        assert_eq!(
            rule()
                .parse(stream("rule foo\n command = bar\n description = baz\n"))
                .unwrap()
                .0,
            Rule::new("foo", "bar", Some("baz".into()))
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

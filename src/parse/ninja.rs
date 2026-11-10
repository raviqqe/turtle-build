use crate::ast::{
    Build, DefaultOutput, DynamicBuild, DynamicModule, Include, Module, Pool, Rule, Statement,
    Submodule, VariableDefinition,
};
use winnow::{
    Parser, Result,
    ascii::{line_ending, space1},
    combinator::{alt, not, opt, peek, preceded, repeat, terminated},
    error::ContextError,
    token::{one_of, take_till, take_while},
};

const OPERATOR_CHARACTERS: [char; 2] = ['|', ':'];
const STRING_LITERAL_DELIMITERS: [char; 7] = [' ', '\t', '\r', '\n', '$', '|', ':'];
const ESCAPED_CHARACTERS: [char; 3] = ['$', ' ', ':'];
const DYNAMIC_MODULE_VERSION_VARIABLE: &str = "ninja_dyndep_version";

pub fn module(input: &mut &str) -> Result<Module> {
    (opt(line_break), repeat(0.., statement), opt(line_break))
        .map(|(_, statements, _)| Module::new(statements))
        .parse_next(input)
}

pub fn dynamic_module(input: &mut &str) -> Result<DynamicModule> {
    (
        opt(line_break),
        dynamic_module_version,
        repeat(0.., dynamic_build),
    )
        .map(|(_, _, builds)| DynamicModule::new(builds))
        .parse_next(input)
}

fn statement(input: &mut &str) -> Result<Statement> {
    alt((
        build.output_into(),
        default.output_into(),
        include.output_into(),
        pool.output_into(),
        rule.output_into(),
        submodule.output_into(),
        variable_definition.output_into(),
    ))
    .parse_next(input)
}

fn variable_definition(input: &mut &str) -> Result<VariableDefinition> {
    (identifier, sign("="), opt(string_line), line_break)
        .map(|(name, _, value, _)| VariableDefinition::new(name.into(), value.unwrap_or_default()))
        .parse_next(input)
}

fn dynamic_module_version(input: &mut &str) -> Result<String> {
    (
        keyword(DYNAMIC_MODULE_VERSION_VARIABLE),
        sign("="),
        string_line,
        line_break,
    )
        .map(|(_, _, version, _)| version)
        .parse_next(input)
}

fn rule(input: &mut &str) -> Result<Rule> {
    (
        keyword("rule"),
        identifier,
        line_break,
        repeat(1.., preceded(indent, variable_definition)),
    )
        .verify_map(
            |(_, name, _, variable_definitions): (_, _, _, Vec<VariableDefinition>)| {
                // TODO Reject unknown rule variables like ninja.
                variable_definitions
                    .iter()
                    .any(|definition| definition.name() == "command")
                    .then(|| Rule::new(name.into(), variable_definitions))
            },
        )
        .parse_next(input)
}

fn pool(input: &mut &str) -> Result<Pool> {
    (
        keyword("pool"),
        identifier,
        line_break,
        repeat(1.., preceded(indent, variable_definition)),
    )
        .verify_map(
            |(_, name, _, variable_definitions): (_, _, _, Vec<VariableDefinition>)| {
                let depth = variable_definitions.last()?;

                variable_definitions
                    .iter()
                    .all(|definition| definition.name() == "depth")
                    .then(|| Pool::new(name.into(), depth.value().into()))
            },
        )
        .parse_next(input)
}

fn build(input: &mut &str) -> Result<Build> {
    (
        keyword("build"),
        repeat(1.., string_literal),
        opt(preceded(sign("|"), repeat(1.., string_literal))),
        sign(":"),
        identifier,
        repeat(0.., string_literal),
        opt(preceded(sign("|"), repeat(1.., string_literal))),
        opt(preceded(sign("||"), repeat(1.., string_literal))),
        line_break,
        repeat(0.., preceded(indent, variable_definition)),
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
                    implicit_outputs.unwrap_or_default(),
                    rule.into(),
                    inputs,
                    implicit_inputs.unwrap_or_default(),
                    order_only_inputs.unwrap_or_default(),
                    variable_definitions,
                )
            },
        )
        .parse_next(input)
}

pub fn dynamic_build(input: &mut &str) -> Result<DynamicBuild> {
    (
        keyword("build"),
        string_literal,
        sign(":"),
        keyword("dyndep"),
        opt(preceded(sign("|"), repeat(1.., string_literal))),
        line_break,
    )
        .map(|(_, output, _, _, implicit_inputs, _)| {
            DynamicBuild::new(output, implicit_inputs.unwrap_or_default())
        })
        .parse_next(input)
}

fn default(input: &mut &str) -> Result<DefaultOutput> {
    (keyword("default"), repeat(1.., string_literal), line_break)
        .map(|(_, outputs, _)| DefaultOutput::new(outputs))
        .parse_next(input)
}

fn include(input: &mut &str) -> Result<Include> {
    (keyword("include"), string_line, line_break)
        .map(|(_, path, _)| Include::new(path))
        .parse_next(input)
}

fn submodule(input: &mut &str) -> Result<Submodule> {
    (keyword("subninja"), string_line, line_break)
        .map(|(_, path, _)| Submodule::new(path))
        .parse_next(input)
}

fn string_line(input: &mut &str) -> Result<String> {
    string(take_till(1.., ['$', '\n']))
        .map(|string| string.trim().to_owned())
        .parse_next(input)
}

fn string_literal(input: &mut &str) -> Result<String> {
    token(string(take_till(1.., STRING_LITERAL_DELIMITERS))).parse_next(input)
}

fn string<'a>(
    text: impl Parser<&'a str, &'a str, ContextError>,
) -> impl Parser<&'a str, String, ContextError> {
    repeat(
        1..,
        alt((
            text,
            ("$", one_of(ESCAPED_CHARACTERS)).take(),
            line_continuation.value(""),
            "$",
        )),
    )
}

fn keyword<'a>(name: &'static str) -> impl Parser<&'a str, (), ContextError> {
    token((name, peek(not(one_of(is_identifier_character))))).void()
}

fn identifier<'a>(input: &mut &'a str) -> Result<&'a str> {
    token(take_while(1.., is_identifier_character)).parse_next(input)
}

const fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
}

fn sign<'a>(sign: &'static str) -> impl Parser<&'a str, (), ContextError> {
    token(terminated(sign, peek(not(one_of(OPERATOR_CHARACTERS))))).void()
}

fn token<'a, O>(
    parser: impl Parser<&'a str, O, ContextError>,
) -> impl Parser<&'a str, O, ContextError> {
    preceded(blank, parser)
}

fn indent(input: &mut &str) -> Result<()> {
    space1.void().parse_next(input)
}

fn blank(input: &mut &str) -> Result<()> {
    repeat(0.., alt((space1.void(), comment, line_continuation))).parse_next(input)
}

fn comment(input: &mut &str) -> Result<()> {
    ("#", take_till(0.., '\n')).void().parse_next(input)
}

fn line_continuation(input: &mut &str) -> Result<()> {
    ("$", line_ending, take_while(0.., ' '))
        .void()
        .parse_next(input)
}

fn line_break(input: &mut &str) -> Result<()> {
    repeat(1.., (blank, line_ending)).parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explicit_build(
        outputs: Vec<String>,
        rule: String,
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
        assert_eq!(module.parse("").unwrap(), Module::new(vec![]));
        assert_eq!(module.parse("#foo\n").unwrap(), Module::new(vec![]));
        assert_eq!(
            module.parse("x = 42\n").unwrap(),
            Module::new(vec![
                VariableDefinition::new("x".into(), "42".into()).into()
            ])
        );
        assert_eq!(
            module.parse("x = 1\ny = 2\n").unwrap(),
            Module::new(vec![
                VariableDefinition::new("x".into(), "1".into()).into(),
                VariableDefinition::new("y".into(), "2".into()).into(),
            ],)
        );
        assert_eq!(
            module.parse("rule foo\n command = bar\n").unwrap(),
            Module::new(vec![
                Rule::new(
                    "foo".into(),
                    vec![VariableDefinition::new("command".into(), "bar".into())]
                )
                .into()
            ])
        );
        assert_eq!(
            module
                .parse("rule foo\n command = bar\nrule baz\n command = blah\n")
                .unwrap(),
            Module::new(vec![
                Rule::new(
                    "foo".into(),
                    vec![VariableDefinition::new("command".into(), "bar".into())]
                )
                .into(),
                Rule::new(
                    "baz".into(),
                    vec![VariableDefinition::new("command".into(), "blah".into())]
                )
                .into(),
            ],)
        );
        assert_eq!(
            module.parse("builddir = foo\n").unwrap(),
            Module::new(vec![
                VariableDefinition::new("builddir".into(), "foo".into()).into()
            ])
        );
        assert_eq!(
            module.parse("default_x = foo\n").unwrap(),
            Module::new(vec![
                VariableDefinition::new("default_x".into(), "foo".into()).into()
            ])
        );
        assert_eq!(
            module.parse("rule-x = foo\n").unwrap(),
            Module::new(vec![
                VariableDefinition::new("rule-x".into(), "foo".into()).into()
            ])
        );
        assert_eq!(
            module.parse("pool foo\n depth = 42\n").unwrap(),
            Module::new(vec![Pool::new("foo".into(), "42".into()).into()])
        );
        assert_eq!(
            module.parse("pool-x = foo\n").unwrap(),
            Module::new(vec![
                VariableDefinition::new("pool-x".into(), "foo".into()).into()
            ])
        );
    }

    #[test]
    fn parse_dynamic_module() {
        assert_eq!(
            dynamic_module.parse("ninja_dyndep_version = 1\n").unwrap(),
            DynamicModule::new(vec![])
        );
        assert_eq!(
            dynamic_module
                .parse("ninja_dyndep_version = 1\nbuild foo: dyndep\n")
                .unwrap(),
            DynamicModule::new(vec![DynamicBuild::new("foo".into(), vec![])])
        );
        assert_eq!(
            dynamic_module
                .parse("ninja_dyndep_version = 1\nbuild foo: dyndep\nbuild bar: dyndep\n")
                .unwrap(),
            DynamicModule::new(vec![
                DynamicBuild::new("foo".into(), vec![]),
                DynamicBuild::new("bar".into(), vec![])
            ])
        );
    }

    #[test]
    fn parse_variable_definition() {
        assert_eq!(
            variable_definition.parse_peek("x = 42\n").unwrap().1,
            VariableDefinition::new("x".into(), "42".into())
        );
        assert_eq!(
            variable_definition.parse_peek("foo = 1 + 1\n").unwrap().1,
            VariableDefinition::new("foo".into(), "1 + 1".into())
        );
        assert_eq!(
            variable_definition.parse_peek("x =\n").unwrap().1,
            VariableDefinition::new("x".into(), "".into())
        );
        assert_eq!(
            variable_definition.parse_peek("x = \n").unwrap().1,
            VariableDefinition::new("x".into(), "".into())
        );
    }

    #[test]
    fn parse_variable_definition_with_line_continuation() {
        assert_eq!(
            variable_definition
                .parse_peek("x = foo $\n    bar\n")
                .unwrap()
                .1,
            VariableDefinition::new("x".into(), "foo bar".into())
        );
        assert_eq!(
            variable_definition
                .parse_peek("x = $\n    foo\n")
                .unwrap()
                .1,
            VariableDefinition::new("x".into(), "foo".into())
        );
        assert_eq!(
            variable_definition.parse_peek("x = foo $\n\n").unwrap().1,
            VariableDefinition::new("x".into(), "foo".into())
        );
    }

    #[test]
    fn parse_dynamic_module_version() {
        assert_eq!(
            dynamic_module_version
                .parse_peek("ninja_dyndep_version = 42\n")
                .unwrap()
                .1,
            "42",
        );
    }

    #[test]
    fn parse_rule() {
        assert_eq!(
            rule.parse_peek("rule foo\n command = bar\n").unwrap().1,
            Rule::new(
                "foo".into(),
                vec![VariableDefinition::new("command".into(), "bar".into())]
            )
        );
        assert_eq!(
            rule.parse_peek("rule foo\n command = bar\n description = baz\n")
                .unwrap()
                .1,
            Rule::new(
                "foo".into(),
                vec![
                    VariableDefinition::new("command".into(), "bar".into()),
                    VariableDefinition::new("description".into(), "baz".into())
                ]
            )
        );
        assert_eq!(
            rule.parse_peek("rule foo\n depfile = foo.d\n command = bar\n deps = gcc\n")
                .unwrap()
                .1,
            Rule::new(
                "foo".into(),
                vec![
                    VariableDefinition::new("depfile".into(), "foo.d".into()),
                    VariableDefinition::new("command".into(), "bar".into()),
                    VariableDefinition::new("deps".into(), "gcc".into())
                ]
            )
        );
        assert_eq!(
            rule.parse_peek("rule foo-bar.baz\n command = blah\n")
                .unwrap()
                .1,
            Rule::new(
                "foo-bar.baz".into(),
                vec![VariableDefinition::new("command".into(), "blah".into())]
            )
        );
    }

    #[test]
    fn reject_rule_without_command() {
        assert!(rule.parse_peek("rule foo\n description = bar\n").is_err());
    }

    #[test]
    fn parse_rule_with_duplicate_command() {
        assert_eq!(
            rule.parse_peek("rule foo\n command = bar\n command = baz\n")
                .unwrap()
                .1,
            Rule::new(
                "foo".into(),
                vec![
                    VariableDefinition::new("command".into(), "bar".into()),
                    VariableDefinition::new("command".into(), "baz".into())
                ]
            )
        );
    }

    #[test]
    fn parse_pool() {
        assert_eq!(
            pool.parse_peek("pool foo\n depth = 42\n").unwrap().1,
            Pool::new("foo".into(), "42".into())
        );
        assert_eq!(
            pool.parse_peek("pool foo-bar.baz\n depth = 42\n")
                .unwrap()
                .1,
            Pool::new("foo-bar.baz".into(), "42".into())
        );
        assert_eq!(
            pool.parse_peek("pool foo\n depth = $x\n").unwrap().1,
            Pool::new("foo".into(), "$x".into())
        );
        assert_eq!(
            pool.parse_peek("pool foo\n # bar\n depth = 42\n")
                .unwrap()
                .1,
            Pool::new("foo".into(), "42".into())
        );
    }

    #[test]
    fn parse_pool_with_duplicate_depth() {
        assert_eq!(
            pool.parse_peek("pool foo\n depth = 1\n depth = 2\n")
                .unwrap()
                .1,
            Pool::new("foo".into(), "2".into())
        );
    }

    #[test]
    fn reject_pool_with_invalid_name() {
        assert!(pool.parse_peek("pool\n depth = 1\n").is_err());
        assert!(pool.parse_peek("pool $foo\n depth = 1\n").is_err());
        assert!(pool.parse_peek("pool foo bar\n depth = 1\n").is_err());
    }

    #[test]
    fn reject_pool_without_depth() {
        assert!(pool.parse_peek("pool foo\n").is_err());
        assert!(module.parse("pool foo\ndepth = 1\n").is_err());
    }

    #[test]
    fn reject_pool_with_unknown_variable() {
        assert!(pool.parse_peek("pool foo\n depth = 1\n bar = 2\n").is_err());
        assert!(module.parse("pool foo\n depth = 1\n bar = 2\n").is_err());
    }

    #[test]
    fn parse_build() {
        assert_eq!(
            build.parse_peek("build foo: bar\n").unwrap().1,
            explicit_build(vec!["foo".into()], "bar".into(), vec![], vec![])
        );
        assert_eq!(
            build.parse_peek("build foo: bar-baz.blah\n").unwrap().1,
            explicit_build(vec!["foo".into()], "bar-baz.blah".into(), vec![], vec![])
        );
        assert_eq!(
            build.parse_peek("build foo: bar baz\n").unwrap().1,
            explicit_build(vec!["foo".into()], "bar".into(), vec!["baz".into()], vec![])
        );
        assert_eq!(
            build.parse_peek("build foo: bar baz blah\n").unwrap().1,
            explicit_build(
                vec!["foo".into()],
                "bar".into(),
                vec!["baz".into(), "blah".into()],
                vec![]
            )
        );
        assert_eq!(
            build.parse_peek("build foo bar: baz\n").unwrap().1,
            explicit_build(
                vec!["foo".into(), "bar".into()],
                "baz".into(),
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build.parse_peek("build foo: bar\n x = 1\n").unwrap().1,
            explicit_build(
                vec!["foo".into()],
                "bar".into(),
                vec![],
                vec![VariableDefinition::new("x".into(), "1".into())]
            )
        );
        assert_eq!(
            build
                .parse_peek("build foo: bar\n x = 1\n y = 2\n")
                .unwrap()
                .1,
            explicit_build(
                vec!["foo".into()],
                "bar".into(),
                vec![],
                vec![
                    VariableDefinition::new("x".into(), "1".into()),
                    VariableDefinition::new("y".into(), "2".into())
                ]
            )
        );
        assert_eq!(
            build.parse_peek("build x1 | x2: rule\n").unwrap().1,
            Build::new(
                vec!["x1".into()],
                vec!["x2".into()],
                "rule".into(),
                vec![],
                vec![],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build.parse_peek("build x1 | x2 x3: rule\n").unwrap().1,
            Build::new(
                vec!["x1".into()],
                vec!["x2".into(), "x3".into()],
                "rule".into(),
                vec![],
                vec![],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build.parse_peek("build x1: rule | x2\n").unwrap().1,
            Build::new(
                vec!["x1".into()],
                vec![],
                "rule".into(),
                vec![],
                vec!["x2".into()],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build.parse_peek("build x1: rule | x2 x3\n").unwrap().1,
            Build::new(
                vec!["x1".into()],
                vec![],
                "rule".into(),
                vec![],
                vec!["x2".into(), "x3".into()],
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build.parse_peek("build x1: rule || x2\n").unwrap().1,
            Build::new(
                vec!["x1".into()],
                vec![],
                "rule".into(),
                vec![],
                vec![],
                vec!["x2".into()],
                vec![],
            )
        );
        assert_eq!(
            build.parse_peek("build x1: rule || x2 x3\n").unwrap().1,
            Build::new(
                vec!["x1".into()],
                vec![],
                "rule".into(),
                vec![],
                vec![],
                vec!["x2".into(), "x3".into()],
                vec![]
            )
        );
    }

    #[test]
    fn parse_build_with_line_continuation() {
        assert_eq!(
            build.parse_peek("build foo $\n    bar: baz\n").unwrap().1,
            explicit_build(
                vec!["foo".into(), "bar".into()],
                "baz".into(),
                vec![],
                vec![]
            )
        );
        assert_eq!(
            build.parse_peek("build foo: $\n    bar\n").unwrap().1,
            explicit_build(vec!["foo".into()], "bar".into(), vec![], vec![])
        );
        assert_eq!(
            build
                .parse_peek("build foo: bar baz $\n    blah\n")
                .unwrap()
                .1,
            explicit_build(
                vec!["foo".into()],
                "bar".into(),
                vec!["baz".into(), "blah".into()],
                vec![]
            )
        );
        assert_eq!(
            build
                .parse_peek("build foo: bar $\n    baz$\n    blah\n")
                .unwrap()
                .1,
            // cspell: disable-next-line
            explicit_build(
                vec!["foo".into()],
                "bar".into(),
                vec!["bazblah".into()], // cspell: disable-line
                vec![]
            )
        );
        assert_eq!(
            build
                .parse_peek("build x1: rule | $\n    x2 || $\n    x3\n")
                .unwrap()
                .1,
            Build::new(
                vec!["x1".into()],
                vec![],
                "rule".into(),
                vec![],
                vec!["x2".into()],
                vec!["x3".into()],
                vec![]
            )
        );
    }

    #[test]
    fn parse_build_with_escaped_character() {
        assert_eq!(
            build
                .parse_peek("build C$:\\foo$ bar: baz C$:\\qux\n")
                .unwrap()
                .1,
            explicit_build(
                vec!["C$:\\foo$ bar".into()],
                "baz".into(),
                vec!["C$:\\qux".into()],
                vec![]
            )
        );
    }

    #[test]
    fn parse_dynamic_build() {
        assert_eq!(
            dynamic_build.parse_peek("build foo: dyndep\n").unwrap().1,
            DynamicBuild::new("foo".into(), vec![])
        );
        assert_eq!(
            dynamic_build
                .parse_peek("build foo: dyndep | bar\n")
                .unwrap()
                .1,
            DynamicBuild::new("foo".into(), vec!["bar".into()])
        );
        assert_eq!(
            dynamic_build
                .parse_peek("build foo: dyndep | bar baz\n")
                .unwrap()
                .1,
            DynamicBuild::new("foo".into(), vec!["bar".into(), "baz".into()])
        );
    }

    #[test]
    fn parse_default() {
        assert!(default.parse_peek("").is_err());
        assert!(default.parse_peek("default\n").is_err());
        assert_eq!(
            default.parse_peek("default foo\n").unwrap().1,
            DefaultOutput::new(vec!["foo".into()])
        );
        assert_eq!(
            default.parse_peek("default foo bar\n").unwrap().1,
            DefaultOutput::new(vec!["foo".into(), "bar".into()])
        );
    }

    #[test]
    fn parse_include() {
        assert_eq!(
            include.parse_peek("include foo\n").unwrap().1,
            Include::new("foo".into())
        );
    }

    #[test]
    fn parse_submodule() {
        assert_eq!(
            submodule.parse_peek("subninja foo\n").unwrap().1,
            Submodule::new("foo".into())
        );
    }

    #[test]
    fn parse_string_line() {
        assert!(string_line.parse_peek("").is_err());
        assert_eq!(string_line.parse_peek("foo").unwrap().1, "foo");
        assert_eq!(string_line.parse_peek("foo\n").unwrap().1, "foo");
        assert_eq!(string_line.parse_peek("foo \n").unwrap().1, "foo");
        assert_eq!(string_line.parse_peek("foo bar").unwrap().1, "foo bar");
    }

    #[test]
    fn parse_string_line_with_line_continuation() {
        assert_eq!(string_line.parse_peek("foo $\n bar").unwrap().1, "foo bar");
        assert_eq!(string_line.parse_peek("foo$\n  bar").unwrap().1, "foobar");
        assert_eq!(string_line.parse_peek("foo$\r\n bar").unwrap().1, "foobar");
        assert_eq!(string_line.parse_peek("foo$\n\tbar").unwrap().1, "foo\tbar");
        assert_eq!(string_line.parse_peek("foo $\n\nbar").unwrap().1, "foo");
        assert_eq!(string_line.parse_peek("foo $").unwrap().1, "foo $");
        assert_eq!(string_line.parse_peek("foo$$\nbar").unwrap().1, "foo$$");
        assert_eq!(
            string_line.parse_peek("foo$$$\n bar").unwrap().1,
            "foo$$bar"
        );
        assert_eq!(string_line.parse_peek("$x$\n $y").unwrap().1, "$x$y");
    }

    #[test]
    fn parse_string_line_with_escaped_character() {
        assert_eq!(
            string_line.parse_peek("foo$ bar$:baz\n").unwrap().1,
            "foo$ bar$:baz"
        );
    }

    #[test]
    fn parse_string_literal() {
        assert!(string_literal.parse_peek("").is_err());
        assert_eq!(string_literal.parse_peek("foo").unwrap().1, "foo");
        assert_eq!(string_literal.parse_peek("foo bar").unwrap().1, "foo");
    }

    #[test]
    fn parse_string_literal_with_line_continuation() {
        assert_eq!(
            string_literal.parse_peek("foo$\n  bar").unwrap().1,
            "foobar"
        );
        assert_eq!(
            string_literal.parse_peek("foo$\n bar baz").unwrap().1,
            "foobar"
        );
        assert_eq!(string_literal.parse_peek("foo $\n bar").unwrap().1, "foo");
        assert_eq!(string_literal.parse_peek(" $\n foo").unwrap().1, "foo");
        assert_eq!(string_literal.parse_peek("foo$$\nbar").unwrap().1, "foo$$");
        assert_eq!(
            string_literal.parse_peek("foo$$$\n bar").unwrap().1,
            "foo$$bar"
        );
    }

    #[test]
    fn parse_string_literal_with_escaped_character() {
        assert_eq!(
            string_literal.parse_peek("foo$ bar baz").unwrap().1,
            "foo$ bar"
        );
        assert_eq!(
            string_literal.parse_peek("C$:\\foo: bar").unwrap().1,
            "C$:\\foo"
        );
        assert_eq!(string_literal.parse_peek("foo$$ bar").unwrap().1, "foo$$");
        assert_eq!(string_literal.parse_peek("foo$$: bar").unwrap().1, "foo$$");
    }

    #[test]
    fn parse_keyword() {
        assert!(keyword("foo").parse_peek("foo").is_ok());
        assert!(keyword("foo").parse_peek("foo bar").is_ok());
        assert!(keyword("fo").parse_peek("foo").is_err());
        assert!(keyword("foo").parse_peek("foo_bar").is_err());
        assert!(keyword("foo").parse_peek("foo.bar").is_err());
        assert!(keyword("foo").parse_peek("foo-bar").is_err());
    }

    #[test]
    fn parse_identifier() {
        assert_eq!(identifier.parse_peek("foo").unwrap().1, "foo");
        assert_eq!(identifier.parse_peek("foo bar").unwrap().1, "foo");
        assert_eq!(identifier.parse_peek("foo_bar").unwrap().1, "foo_bar");
        assert_eq!(identifier.parse_peek("_foo").unwrap().1, "_foo");
        assert_eq!(identifier.parse_peek("foo.bar").unwrap().1, "foo.bar");
        assert_eq!(identifier.parse_peek("foo-bar").unwrap().1, "foo-bar");
        assert_eq!(identifier.parse_peek("42").unwrap().1, "42");
        assert_eq!(identifier.parse_peek("foo:bar").unwrap().1, "foo");
        assert!(identifier.parse_peek("").is_err());
    }

    #[test]
    fn parse_blank() {
        assert!(blank.parse("").is_ok());
        assert!(blank.parse(" ").is_ok());
        assert!(blank.parse("\t").is_ok());
        assert!(blank.parse("  ").is_ok());
        assert!(blank.parse(" \t").is_ok());
        assert!(blank.parse("#").is_ok());
        assert!(blank.parse("#foo").is_ok());
        assert!(blank.parse(" #foo").is_ok());
        assert!(blank.parse("$\n").is_ok());
        assert!(blank.parse(" $\n ").is_ok());
        assert!(blank.parse("$\n$\r\n").is_ok());
        assert!(blank.parse("\n").is_err());
        assert!(blank.parse(" \n").is_err());
        assert!(blank.parse("$").is_err());
        assert!(blank.parse("$$\n").is_err());
    }

    #[test]
    fn parse_line_continuation() {
        assert!(line_continuation.parse("$\n").is_ok());
        assert!(line_continuation.parse("$\r\n").is_ok());
        assert!(line_continuation.parse("$\n  ").is_ok());
        assert!(line_continuation.parse("").is_err());
        assert!(line_continuation.parse("$").is_err());
        assert!(line_continuation.parse("$ \n").is_err());
        assert!(line_continuation.parse("$\n\t").is_err());
    }

    #[test]
    fn parse_line_break() {
        assert!(line_break.parse("").is_err());
        assert!(line_break.parse("\n").is_ok());
        assert!(line_break.parse("\r\n").is_ok());
        assert!(line_break.parse(" \n").is_ok());
        assert!(line_break.parse("  \n").is_ok());
        assert!(line_break.parse("\n\n").is_ok());
        assert!(line_break.parse(" $\n\n").is_ok());
        assert!(line_break.parse("#foo $\n").is_ok());
        assert!(line_break.parse("\n ").is_err());
        assert!(line_break.parse("$\n").is_err());
    }
}

use super::error::DepfileError;

pub fn parse(path: &str, source: &str) -> Result<Vec<String>, DepfileError> {
    let mut parser = Parser::new(path, source);
    let mut dependencies = vec![];

    parser.skip_blank();

    while parser.has_content() {
        loop {
            let Some(token) = parser.read_token()? else {
                return Err(parser.error("expected ':'"));
            };

            if token.terminated_by_colon {
                break;
            }
        }

        // A depfile may list many rules (like GCC's -MP phony targets for
        // headers). here we harvest dependencies from every rule *regardless* of its
        // target rather than only the one matching this build's output. That
        // over-approximates the dependency set, which is only ever going to cause an
        // extra, safe rebuild.
        while let Some(token) = parser.read_token()? {
            dependencies.push(token.text);
        }

        parser.skip_blank();
    }

    Ok(dependencies)
}

struct Token {
    text: String,
    terminated_by_colon: bool,
}

struct Parser<'a> {
    path: &'a str,
    source: &'a str,
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(path: &'a str, source: &'a str) -> Self {
        Self {
            path,
            source,
            index: 0,
        }
    }

    fn error(&self, message: impl Into<String>) -> DepfileError {
        let (line, column) = self.position();

        DepfileError::new(self.path, line, column, message)
    }

    fn position(&self) -> (usize, usize) {
        let consumed = &self.source[..self.index];

        (
            consumed.matches('\n').count() + 1,
            consumed
                .rsplit('\n')
                .next()
                .unwrap_or(consumed)
                .chars()
                .count()
                + 1,
        )
    }

    fn peek(&self) -> Option<char> {
        self.source[self.index..].chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        let mut characters = self.source[self.index..].chars();

        characters.next()?;
        characters.next()
    }

    fn advance(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.index += character.len_utf8();
        Some(character)
    }

    fn has_content(&self) -> bool {
        self.peek().is_some()
    }

    // Skips blank lines and whole-line comments so that every call site can
    // assume it starts right at the first meaningful character of a rule.
    fn skip_blank(&mut self) {
        loop {
            while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
                self.advance();
            }

            if self.peek() == Some('#') {
                while !matches!(self.peek(), None | Some('\n')) {
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn skip_inline_spaces(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t') => {
                    self.advance();
                }
                Some('\\') if matches!(self.peek_next(), Some('\n' | '\r')) => {
                    self.advance();

                    if matches!(self.peek(), Some('\r')) {
                        self.advance();
                    }

                    if matches!(self.peek(), Some('\n')) {
                        self.advance();
                    }
                }
                _ => return,
            }
        }
    }

    fn read_token(&mut self) -> Result<Option<Token>, DepfileError> {
        self.skip_inline_spaces();

        let mut text = String::new();
        let mut terminated_by_colon = false;

        loop {
            match self.peek() {
                None | Some(' ' | '\t' | '\n' | '\r') => break,
                Some('\\') => match self.peek_next() {
                    Some(' ' | '\t' | ':' | '#') => {
                        self.advance();
                        text.push(self.advance().unwrap());
                    }
                    Some('\n' | '\r') => break,
                    _ => text.push(self.advance().unwrap()),
                },
                Some('$') if self.peek_next() == Some('$') => {
                    self.advance();
                    self.advance();
                    text.push('$');
                }
                Some(':') => match self.peek_next() {
                    None | Some(' ' | '\t' | '\n' | '\r') => {
                        self.advance();
                        terminated_by_colon = true;
                        break;
                    }
                    _ => text.push(self.advance().unwrap()),
                },
                Some(character) => {
                    text.push(character);
                    self.advance();
                }
            }
        }

        Ok(if text.is_empty() && !terminated_by_colon {
            None
        } else {
            Some(Token {
                text,
                terminated_by_colon,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parse(source: &str) -> Result<Vec<String>, DepfileError> {
        super::parse("foo.d", source)
    }

    #[test]
    fn parse_dependencies() {
        assert_eq!(
            parse("foo.o: foo.c foo.h\n").unwrap(),
            vec!["foo.c", "foo.h"]
        );
        assert_eq!(
            parse("foo.o: foo.c \\\n bar.h\n").unwrap(),
            vec!["foo.c", "bar.h"]
        );
        assert_eq!(parse("foo.o : foo.c\r\n").unwrap(), vec!["foo.c"]);
        assert_eq!(parse("foo.o: C:/foo.c\n").unwrap(), vec!["C:/foo.c"]);
        assert_eq!(parse("foo.o:\nbar.o: bar.h\n").unwrap(), vec!["bar.h"]);
    }

    #[test]
    fn parse_multiple_targets() {
        assert_eq!(parse("foo.o bar.o: dep.h\n").unwrap(), vec!["dep.h"]);
    }

    #[test]
    fn parse_escaped_space() {
        assert_eq!(parse("foo.o: a\\ b.h\n").unwrap(), vec!["a b.h"]);
    }

    #[test]
    fn parse_comment_line() {
        assert_eq!(parse("foo.o: foo.c\n# comment\n").unwrap(), vec!["foo.c"]);
        assert_eq!(
            parse("# leading comment\nfoo.o: foo.c\n").unwrap(),
            vec!["foo.c"]
        );
    }

    #[test]
    fn parse_target_without_space_before_colon() {
        assert!(parse("foo.o:foo.c\n").is_err());
    }

    #[test]
    fn parse_colon_not_followed_by_whitespace() {
        assert!(parse("foo.o :foo.c\n").is_err());
    }

    #[test]
    fn parse_missing_colon() {
        assert!(parse("foo.o foo.c\n").is_err());
    }

    #[test]
    fn parse_escaped_dollar() {
        assert_eq!(parse("foo.o: a $$b.h\n").unwrap(), vec!["a", "$b.h"]);
    }

    #[test]
    fn parse_error_location() {
        let error = parse("foo.o foo.c\n").unwrap_err();

        assert_eq!(error, DepfileError::new("foo.d", 1, 12, "expected ':'"));
    }
}

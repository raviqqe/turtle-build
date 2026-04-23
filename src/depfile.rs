pub fn parse(source: &str) -> Result<Vec<String>, String> {
    let mut parser = Parser::new(source);
    let mut dependencies = vec![];

    while parser.skip_line_breaks() {
        let Some(target) = parser.read_path()? else {
            break;
        };

        parser.skip_spaces()?;

        if !target.ends_with(':') {
            parser.expect(':')?;
        }

        while let Some(path) = parser.read_path()? {
            dependencies.push(path.into());
        }
    }

    Ok(dependencies)
}

struct Parser<'a> {
    source: &'a str,
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, index: 0 }
    }

    fn expect(&mut self, character: char) -> Result<(), String> {
        match self.peek() {
            Some(peek) if peek == character => {
                self.advance();
                Ok(())
            }
            _ => Err(format!("expected '{character}'")),
        }
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

    fn skip_line_breaks(&mut self) -> bool {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.advance();
        }

        self.peek().is_some()
    }

    fn skip_spaces(&mut self) -> Result<(), String> {
        loop {
            match self.peek() {
                Some(' ' | '\t') => {
                    self.advance();
                }
                Some('\\') if matches!(self.peek_next(), Some('\n')) => {
                    self.advance();
                    self.advance();
                }
                Some('\\') if matches!(self.peek_next(), Some('\r')) => {
                    self.advance();
                    self.advance();

                    if matches!(self.peek(), Some('\n')) {
                        self.advance();
                    }
                }
                Some('\\') => return Err("invalid backslash escape".into()),
                _ => return Ok(()),
            }
        }
    }

    fn read_path(&mut self) -> Result<Option<&'a str>, String> {
        self.skip_spaces()?;

        let start = self.index;

        loop {
            match self.peek() {
                None | Some(' ' | '\t' | '\n' | '\r') => break,
                Some('\\')
                    if matches!(self.peek_next(), Some('\n'))
                        || matches!(self.peek_next(), Some('\r')) =>
                {
                    break;
                }
                _ => {
                    self.advance();
                }
            }
        }

        if self.index == start {
            Ok(None)
        } else {
            Ok(Some(&self.source[start..self.index]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

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
}

use crate::Error;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expression {
    Integer(i64),
    String(String),
    Symbol(String),
    List(Vec<Expression>),
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    LeftParenthesis,
    RightParenthesis,
    LeftBracket,
    RightBracket,
    Atom(String),
    String(String),
}

pub(crate) fn parse(source: &str) -> Result<Vec<Expression>, Error> {
    let tokens = tokenize(source)?;
    let mut parser = Parser {
        tokens: &tokens,
        position: 0,
    };
    let mut expressions = Vec::new();
    while parser.position != tokens.len() {
        expressions.push(parser.expression()?);
    }
    if expressions.is_empty() {
        return Err(Error::new("expected a top-level form"));
    }
    Ok(expressions)
}

fn tokenize(source: &str) -> Result<Vec<Token>, Error> {
    let mut tokens = Vec::new();
    let mut characters = source.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '(' => tokens.push(Token::LeftParenthesis),
            ')' => tokens.push(Token::RightParenthesis),
            '[' => tokens.push(Token::LeftBracket),
            ']' => tokens.push(Token::RightBracket),
            ';' => {
                for character in characters.by_ref() {
                    if character == '\n' {
                        break;
                    }
                }
            }
            '"' => tokens.push(Token::String(read_string(&mut characters)?)),
            character if character.is_whitespace() => {}
            first => {
                let mut atom = String::from(first);
                while let Some(&next) = characters.peek() {
                    if next.is_whitespace() || matches!(next, '(' | ')' | '[' | ']' | ';') {
                        break;
                    }
                    atom.push(next);
                    characters.next();
                }
                tokens.push(Token::Atom(atom));
            }
        }
    }

    Ok(tokens)
}

fn read_string(
    characters: &mut std::iter::Peekable<impl Iterator<Item = char>>,
) -> Result<String, Error> {
    let mut value = String::new();
    while let Some(character) = characters.next() {
        match character {
            '"' => return Ok(value),
            '\\' => {
                let escaped = characters
                    .next()
                    .ok_or_else(|| Error::new("unterminated escape sequence in string"))?;
                value.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    other => {
                        return Err(Error::new(format!("unsupported string escape `\\{other}`")));
                    }
                });
            }
            other => value.push(other),
        }
    }
    Err(Error::new("unterminated string literal"))
}

struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl Parser<'_> {
    fn expression(&mut self) -> Result<Expression, Error> {
        let token = self
            .tokens
            .get(self.position)
            .ok_or_else(|| Error::new("expected an expression"))?
            .clone();
        self.position += 1;

        match token {
            Token::LeftParenthesis => self.list(Token::RightParenthesis, "unterminated list"),
            Token::LeftBracket => {
                self.list(Token::RightBracket, "unterminated generic parameter list")
            }
            Token::RightParenthesis => Err(Error::new("unexpected `)`")),
            Token::RightBracket => Err(Error::new("unexpected `]`")),
            Token::String(value) => Ok(Expression::String(value)),
            Token::Atom(atom) => match atom.parse::<i64>() {
                Ok(integer) => Ok(Expression::Integer(integer)),
                Err(_) => Ok(Expression::Symbol(atom)),
            },
        }
    }

    fn list(&mut self, closing: Token, unterminated: &str) -> Result<Expression, Error> {
        let mut expressions = Vec::new();
        loop {
            match self.tokens.get(self.position) {
                Some(token) if *token == closing => {
                    self.position += 1;
                    return Ok(Expression::List(expressions));
                }
                Some(_) => expressions.push(self.expression()?),
                None => return Err(Error::new(unterminated)),
            }
        }
    }
}

use crate::Error;
use crate::syntax::Expression;

#[derive(Debug)]
pub(crate) struct Program {
    pub(crate) output: Vec<Vec<u8>>,
    pub(crate) exit_code: i64,
}

#[derive(Clone, Debug, PartialEq)]
enum Type {
    Int,
    String,
    Unit,
    Array(Box<Type>),
}

pub(crate) fn analyze(expression: Expression) -> Result<Program, Error> {
    let definition = list(&expression, "program must contain a `defn` form")?;
    expect_symbol(definition.first(), "defn", "program must start with `defn`")?;
    expect_symbol(
        definition.get(1),
        "main",
        "entry function must be named `main`",
    )?;
    check_parameters(definition.get(2))?;
    expect_symbol(
        definition.get(3),
        "->",
        "expected `->` before the return type",
    )?;
    let return_type = parse_type(
        definition
            .get(4)
            .ok_or_else(|| Error::new("main is missing a return type"))?,
    )?;
    if return_type != Type::Int {
        return Err(Error::new("main must return `Int`"));
    }
    if definition.len() != 6 {
        return Err(Error::new("main must contain exactly one body expression"));
    }

    let mut output = Vec::new();
    let (body_type, exit_code) = analyze_body(&definition[5], &mut output)?;
    if body_type != Type::Int {
        return Err(Error::new("main body must evaluate to `Int`"));
    }

    Ok(Program {
        output,
        exit_code: exit_code.ok_or_else(|| Error::new("main must end in an integer expression"))?,
    })
}

fn check_parameters(expression: Option<&Expression>) -> Result<(), Error> {
    let parameters = list(
        expression.ok_or_else(|| Error::new("main is missing its parameter list"))?,
        "main parameters must be a list",
    )?;
    if parameters.len() != 1 {
        return Err(Error::new(
            "main must accept exactly one `Array String` parameter",
        ));
    }
    let parameter = list(&parameters[0], "main parameter must be `(name type)`")?;
    if parameter.len() != 2 || !matches!(parameter[0], Expression::Symbol(_)) {
        return Err(Error::new("main parameter must be `(name type)`"));
    }
    if parse_type(&parameter[1])? != Type::Array(Box::new(Type::String)) {
        return Err(Error::new("main parameter must have type `(Array String)`"));
    }
    Ok(())
}

fn analyze_body(
    expression: &Expression,
    output: &mut Vec<Vec<u8>>,
) -> Result<(Type, Option<i64>), Error> {
    match expression {
        Expression::Integer(value) => Ok((Type::Int, Some(*value))),
        Expression::String(_) => Ok((Type::String, None)),
        Expression::Symbol(symbol) => Err(Error::new(format!(
            "unsupported expression `{symbol}` in main"
        ))),
        Expression::List(expressions) => {
            let Some(Expression::Symbol(operator)) = expressions.first() else {
                return Err(Error::new("expression must begin with an operator"));
            };
            match operator.as_str() {
                "do" => analyze_do(&expressions[1..], output),
                "print" => analyze_print(&expressions[1..], output),
                other => Err(Error::new(format!("unsupported operation `{other}`"))),
            }
        }
    }
}

fn analyze_do(
    expressions: &[Expression],
    output: &mut Vec<Vec<u8>>,
) -> Result<(Type, Option<i64>), Error> {
    if expressions.is_empty() {
        return Err(Error::new("`do` requires at least one expression"));
    }
    let mut result = (Type::Unit, None);
    for expression in expressions {
        result = analyze_body(expression, output)?;
    }
    Ok(result)
}

fn analyze_print(
    arguments: &[Expression],
    output: &mut Vec<Vec<u8>>,
) -> Result<(Type, Option<i64>), Error> {
    let [Expression::String(value)] = arguments else {
        return Err(Error::new("`print` requires exactly one string literal"));
    };
    output.push(value.as_bytes().to_vec());
    Ok((Type::Unit, None))
}

fn parse_type(expression: &Expression) -> Result<Type, Error> {
    match expression {
        Expression::Symbol(name) if name == "Int" => Ok(Type::Int),
        Expression::Symbol(name) if name == "String" => Ok(Type::String),
        Expression::Symbol(name) if name == "Unit" => Ok(Type::Unit),
        Expression::List(elements)
            if elements.len() == 2
                && matches!(&elements[0], Expression::Symbol(name) if name == "Array") =>
        {
            Ok(Type::Array(Box::new(parse_type(&elements[1])?)))
        }
        _ => Err(Error::new("unknown type")),
    }
}

fn list<'a>(expression: &'a Expression, message: &str) -> Result<&'a [Expression], Error> {
    match expression {
        Expression::List(expressions) => Ok(expressions),
        _ => Err(Error::new(message)),
    }
}

fn expect_symbol(
    expression: Option<&Expression>,
    expected: &str,
    message: &str,
) -> Result<(), Error> {
    match expression {
        Some(Expression::Symbol(actual)) if actual == expected => Ok(()),
        _ => Err(Error::new(message)),
    }
}

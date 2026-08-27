use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::Error;
use crate::syntax::Expression;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Type {
    Int,
    String,
    Unit,
    Array(Box<Type>),
    Parameter(String),
    TraitSelf,
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int => write!(formatter, "Int"),
            Self::String => write!(formatter, "String"),
            Self::Unit => write!(formatter, "Unit"),
            Self::Array(element) => write!(formatter, "(Array {element})"),
            Self::Parameter(name) => name.fmt(formatter),
            Self::TraitSelf => write!(formatter, "Self"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Program {
    pub(crate) strings: Vec<Vec<u8>>,
    pub(crate) functions: Vec<Function>,
    pub(crate) main: usize,
}

#[derive(Debug)]
pub(crate) struct Function {
    pub(crate) symbol: String,
    pub(crate) parameters: Vec<Type>,
    pub(crate) return_type: Type,
    pub(crate) body: TypedExpression,
}

#[derive(Debug)]
pub(crate) struct TypedExpression {
    pub(crate) ty: Type,
    pub(crate) kind: ExpressionKind,
}

#[derive(Debug)]
pub(crate) enum ExpressionKind {
    Integer(i64),
    String,
    Parameter(usize),
    Do(Vec<TypedExpression>),
    Print(usize),
    Add(Box<TypedExpression>, Box<TypedExpression>),
    Call {
        function: usize,
        arguments: Vec<TypedExpression>,
    },
}

#[derive(Clone, Debug)]
struct Parameter {
    name: String,
    ty: Type,
}

#[derive(Clone, Debug)]
struct FunctionDefinition {
    name: String,
    type_parameters: Vec<String>,
    constraints: Vec<Constraint>,
    parameters: Vec<Parameter>,
    return_type: Type,
    body: Expression,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Constraint {
    parameter: String,
    trait_name: String,
}

#[derive(Clone, Debug)]
struct MethodSignature {
    name: String,
    parameters: Vec<Parameter>,
    return_type: Type,
}

#[derive(Clone, Debug)]
struct TraitDefinition {
    name: String,
    methods: Vec<MethodSignature>,
}

#[derive(Clone, Debug)]
struct ImplDefinition {
    trait_name: String,
    for_type: Type,
    methods: Vec<FunctionDefinition>,
}

#[derive(Default)]
struct Declarations {
    traits: Vec<TraitDefinition>,
    implementations: Vec<ImplDefinition>,
    functions: Vec<FunctionDefinition>,
}

#[derive(Clone)]
struct ConcreteDefinition {
    definition: FunctionDefinition,
    function: usize,
}

#[derive(Clone)]
struct ImplMethod {
    trait_name: String,
    for_type: Type,
    signature: MethodSignature,
    definition: FunctionDefinition,
    function: usize,
}

struct PendingFunction {
    symbol: String,
    parameters: Vec<Type>,
    return_type: Type,
    body: Option<TypedExpression>,
}

struct Analyzer {
    traits: HashMap<String, TraitDefinition>,
    concrete_functions: HashMap<String, ConcreteDefinition>,
    generic_functions: HashMap<String, FunctionDefinition>,
    impl_methods: Vec<ImplMethod>,
    pending_functions: Vec<PendingFunction>,
    specializations: HashMap<(String, Vec<Type>), usize>,
    strings: Vec<Vec<u8>>,
}

pub(crate) fn analyze(expressions: Vec<Expression>) -> Result<Program, Error> {
    let declarations = parse_declarations(expressions)?;
    Analyzer::new(declarations)?.analyze()
}

impl Analyzer {
    fn new(declarations: Declarations) -> Result<Self, Error> {
        let mut traits = HashMap::new();
        for trait_definition in declarations.traits {
            if traits
                .insert(trait_definition.name.clone(), trait_definition)
                .is_some()
            {
                return Err(Error::new("trait names must be unique"));
            }
        }

        let mut analyzer = Self {
            traits,
            concrete_functions: HashMap::new(),
            generic_functions: HashMap::new(),
            impl_methods: Vec::new(),
            pending_functions: Vec::new(),
            specializations: HashMap::new(),
            strings: Vec::new(),
        };

        for definition in declarations.functions {
            if analyzer.concrete_functions.contains_key(&definition.name)
                || analyzer.generic_functions.contains_key(&definition.name)
            {
                return Err(Error::new(format!(
                    "function `{}` is defined more than once",
                    definition.name
                )));
            }
            if definition.type_parameters.is_empty() {
                let function = analyzer.reserve_function(
                    if definition.name == "main" {
                        "main".to_owned()
                    } else {
                        format!("kal.fn.{}", definition.name)
                    },
                    definition
                        .parameters
                        .iter()
                        .map(|item| item.ty.clone())
                        .collect(),
                    definition.return_type.clone(),
                );
                analyzer.concrete_functions.insert(
                    definition.name.clone(),
                    ConcreteDefinition {
                        definition,
                        function,
                    },
                );
            } else {
                analyzer
                    .generic_functions
                    .insert(definition.name.clone(), definition);
            }
        }

        analyzer.register_implementations(declarations.implementations)?;
        Ok(analyzer)
    }

    fn register_implementations(
        &mut self,
        implementations: Vec<ImplDefinition>,
    ) -> Result<(), Error> {
        let mut implemented = HashSet::new();
        for implementation in implementations {
            let trait_definition = self
                .traits
                .get(&implementation.trait_name)
                .ok_or_else(|| {
                    Error::new(format!(
                        "unknown trait `{}` in implementation",
                        implementation.trait_name
                    ))
                })?
                .clone();
            if !implemented.insert((
                implementation.trait_name.clone(),
                implementation.for_type.clone(),
            )) {
                return Err(Error::new(format!(
                    "trait `{}` is implemented more than once for `{}`",
                    implementation.trait_name, implementation.for_type
                )));
            }

            let mut methods = HashMap::new();
            for method in implementation.methods {
                let method_name = method.name.clone();
                if methods.insert(method_name.clone(), method).is_some() {
                    return Err(Error::new(format!(
                        "method `{method_name}` is implemented more than once"
                    )));
                }
            }

            for signature in &trait_definition.methods {
                let definition = methods.remove(&signature.name).ok_or_else(|| {
                    Error::new(format!(
                        "implementation of `{}` for `{}` is missing method `{}`",
                        implementation.trait_name, implementation.for_type, signature.name
                    ))
                })?;
                let expected_parameters: Vec<_> = signature
                    .parameters
                    .iter()
                    .map(|parameter| replace_self(&parameter.ty, &implementation.for_type))
                    .collect();
                let actual_parameters: Vec<_> = definition
                    .parameters
                    .iter()
                    .map(|parameter| parameter.ty.clone())
                    .collect();
                let expected_return =
                    replace_self(&signature.return_type, &implementation.for_type);
                if actual_parameters != expected_parameters
                    || definition.return_type != expected_return
                {
                    return Err(Error::new(format!(
                        "method `{}` does not match trait `{}` for `{}`",
                        signature.name, implementation.trait_name, implementation.for_type
                    )));
                }

                let function = self.reserve_function(
                    format!(
                        "kal.impl.{}.{}.{}",
                        implementation.trait_name, implementation.for_type, signature.name
                    ),
                    actual_parameters,
                    definition.return_type.clone(),
                );
                self.impl_methods.push(ImplMethod {
                    trait_name: implementation.trait_name.clone(),
                    for_type: implementation.for_type.clone(),
                    signature: signature.clone(),
                    definition,
                    function,
                });
            }
            if let Some(extra) = methods.keys().next() {
                return Err(Error::new(format!(
                    "method `{extra}` is not declared by trait `{}`",
                    implementation.trait_name
                )));
            }
        }
        Ok(())
    }

    fn analyze(mut self) -> Result<Program, Error> {
        let main = self
            .concrete_functions
            .get("main")
            .ok_or_else(|| Error::new("program must define `main`"))?
            .clone();
        check_main_signature(&main.definition)?;

        for definition in self.generic_functions.values() {
            self.validate_generic_function(definition)?;
        }

        let concrete: Vec<_> = self.concrete_functions.values().cloned().collect();
        let implementation_methods = self.impl_methods.clone();
        for definition in concrete {
            self.lower_definition(&definition.definition, definition.function, &HashMap::new())?;
        }
        for method in implementation_methods {
            self.lower_definition(&method.definition, method.function, &HashMap::new())?;
        }

        let functions = self
            .pending_functions
            .into_iter()
            .map(|function| {
                Ok(Function {
                    symbol: function.symbol,
                    parameters: function.parameters,
                    return_type: function.return_type,
                    body: function
                        .body
                        .ok_or_else(|| Error::new("compiler did not lower a function body"))?,
                })
            })
            .collect::<Result<_, Error>>()?;

        Ok(Program {
            strings: self.strings,
            functions,
            main: main.function,
        })
    }

    fn validate_generic_function(&self, definition: &FunctionDefinition) -> Result<(), Error> {
        for constraint in &definition.constraints {
            if !self.traits.contains_key(&constraint.trait_name) {
                return Err(Error::new(format!(
                    "unknown trait `{}` in requirements for `{}`",
                    constraint.trait_name, definition.name
                )));
            }
        }
        let bindings = definition
            .parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter.ty.clone()))
            .collect();
        let body_type =
            self.validate_expression(&definition.body, &bindings, &definition.constraints)?;
        if body_type != definition.return_type {
            return Err(Error::new(format!(
                "function `{}` body has type `{body_type}`, expected `{}`",
                definition.name, definition.return_type
            )));
        }
        Ok(())
    }

    fn validate_expression(
        &self,
        expression: &Expression,
        bindings: &HashMap<String, Type>,
        constraints: &[Constraint],
    ) -> Result<Type, Error> {
        match expression {
            Expression::Integer(_) => Ok(Type::Int),
            Expression::String(_) => Ok(Type::String),
            Expression::Symbol(name) => bindings
                .get(name)
                .cloned()
                .ok_or_else(|| Error::new(format!("unknown name `{name}`"))),
            Expression::List(expressions) => {
                let operator = first_symbol(expressions, "expression must begin with an operator")?;
                match operator {
                    "do" => {
                        if expressions.len() == 1 {
                            return Err(Error::new("`do` requires at least one expression"));
                        }
                        let mut ty = Type::Unit;
                        for expression in &expressions[1..] {
                            ty = self.validate_expression(expression, bindings, constraints)?;
                        }
                        Ok(ty)
                    }
                    "print" => {
                        let [Expression::String(_)] = &expressions[1..] else {
                            return Err(Error::new("`print` requires exactly one string literal"));
                        };
                        Ok(Type::Unit)
                    }
                    "+" => {
                        let [left, right] = &expressions[1..] else {
                            return Err(Error::new("`+` requires exactly two arguments"));
                        };
                        let left = self.validate_expression(left, bindings, constraints)?;
                        let right = self.validate_expression(right, bindings, constraints)?;
                        if left != Type::Int || right != Type::Int {
                            return Err(Error::new("`+` requires two `Int` arguments"));
                        }
                        Ok(Type::Int)
                    }
                    name => {
                        let argument_types = expressions[1..]
                            .iter()
                            .map(|argument| {
                                self.validate_expression(argument, bindings, constraints)
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        self.validate_call(name, &argument_types, constraints)
                    }
                }
            }
        }
    }

    fn validate_call(
        &self,
        name: &str,
        arguments: &[Type],
        constraints: &[Constraint],
    ) -> Result<Type, Error> {
        if let Some(function) = self.concrete_functions.get(name) {
            let parameters: Vec<_> = function
                .definition
                .parameters
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect();
            check_arguments(name, arguments, &parameters)?;
            return Ok(function.definition.return_type.clone());
        }
        if let Some(function) = self.generic_functions.get(name) {
            let substitutions = infer_type_arguments(function, arguments)?;
            self.check_requirements(function, &substitutions, constraints)?;
            return Ok(substitute(&function.return_type, &substitutions));
        }

        let mut matches = Vec::new();
        for trait_definition in self.traits.values() {
            for method in trait_definition
                .methods
                .iter()
                .filter(|method| method.name == name)
            {
                if let Some(self_type) = match_trait_signature(method, arguments) {
                    let available = match &self_type {
                        Type::Parameter(parameter) => constraints.iter().any(|constraint| {
                            constraint.parameter == *parameter
                                && constraint.trait_name == trait_definition.name
                        }),
                        concrete => self.has_impl(&trait_definition.name, concrete),
                    };
                    if available {
                        matches.push(replace_self(&method.return_type, &self_type));
                    }
                }
            }
        }
        match matches.as_slice() {
            [return_type] => Ok(return_type.clone()),
            [] => Err(Error::new(format!(
                "no function or trait method `{name}` accepts the supplied argument types"
            ))),
            _ => Err(Error::new(format!(
                "call to trait method `{name}` is ambiguous"
            ))),
        }
    }

    fn lower_definition(
        &mut self,
        definition: &FunctionDefinition,
        function: usize,
        substitutions: &HashMap<String, Type>,
    ) -> Result<(), Error> {
        let bindings = definition
            .parameters
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                (
                    parameter.name.clone(),
                    (substitute(&parameter.ty, substitutions), index),
                )
            })
            .collect();
        let body = self.lower_expression(&definition.body, &bindings)?;
        let return_type = substitute(&definition.return_type, substitutions);
        if body.ty != return_type {
            let description = if definition.name == "main" {
                "main body".to_owned()
            } else {
                format!("function `{}` body", definition.name)
            };
            return Err(Error::new(format!(
                "{description} has type `{}`, expected `{return_type}`",
                body.ty
            )));
        }
        self.pending_functions[function].body = Some(body);
        Ok(())
    }

    fn lower_expression(
        &mut self,
        expression: &Expression,
        bindings: &HashMap<String, (Type, usize)>,
    ) -> Result<TypedExpression, Error> {
        match expression {
            Expression::Integer(value) => Ok(typed(Type::Int, ExpressionKind::Integer(*value))),
            Expression::String(_) => Ok(typed(Type::String, ExpressionKind::String)),
            Expression::Symbol(name) => {
                let (ty, index) = bindings
                    .get(name)
                    .ok_or_else(|| Error::new(format!("unknown name `{name}`")))?;
                Ok(typed(ty.clone(), ExpressionKind::Parameter(*index)))
            }
            Expression::List(expressions) => {
                let operator = first_symbol(expressions, "expression must begin with an operator")?;
                match operator {
                    "do" => self.lower_do(&expressions[1..], bindings),
                    "print" => self.lower_print(&expressions[1..]),
                    "+" => self.lower_add(&expressions[1..], bindings),
                    name => self.lower_call(name, &expressions[1..], bindings),
                }
            }
        }
    }

    fn lower_do(
        &mut self,
        expressions: &[Expression],
        bindings: &HashMap<String, (Type, usize)>,
    ) -> Result<TypedExpression, Error> {
        if expressions.is_empty() {
            return Err(Error::new("`do` requires at least one expression"));
        }
        let expressions = expressions
            .iter()
            .map(|expression| self.lower_expression(expression, bindings))
            .collect::<Result<Vec<_>, _>>()?;
        let ty = expressions.last().expect("non-empty do").ty.clone();
        Ok(typed(ty, ExpressionKind::Do(expressions)))
    }

    fn lower_print(&mut self, arguments: &[Expression]) -> Result<TypedExpression, Error> {
        let [Expression::String(value)] = arguments else {
            return Err(Error::new("`print` requires exactly one string literal"));
        };
        let index = self.strings.len();
        self.strings.push(value.as_bytes().to_vec());
        Ok(typed(Type::Unit, ExpressionKind::Print(index)))
    }

    fn lower_add(
        &mut self,
        arguments: &[Expression],
        bindings: &HashMap<String, (Type, usize)>,
    ) -> Result<TypedExpression, Error> {
        let [left, right] = arguments else {
            return Err(Error::new("`+` requires exactly two arguments"));
        };
        let left = self.lower_expression(left, bindings)?;
        let right = self.lower_expression(right, bindings)?;
        if left.ty != Type::Int || right.ty != Type::Int {
            return Err(Error::new("`+` requires two `Int` arguments"));
        }
        Ok(typed(
            Type::Int,
            ExpressionKind::Add(Box::new(left), Box::new(right)),
        ))
    }

    fn lower_call(
        &mut self,
        name: &str,
        argument_expressions: &[Expression],
        bindings: &HashMap<String, (Type, usize)>,
    ) -> Result<TypedExpression, Error> {
        let arguments = argument_expressions
            .iter()
            .map(|argument| self.lower_expression(argument, bindings))
            .collect::<Result<Vec<_>, _>>()?;
        let argument_types: Vec<_> = arguments
            .iter()
            .map(|argument| argument.ty.clone())
            .collect();

        if let Some(concrete) = self.concrete_functions.get(name).cloned() {
            let parameters: Vec<_> = concrete
                .definition
                .parameters
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect();
            check_arguments(name, &argument_types, &parameters)?;
            return Ok(typed(
                concrete.definition.return_type,
                ExpressionKind::Call {
                    function: concrete.function,
                    arguments,
                },
            ));
        }
        if let Some(generic) = self.generic_functions.get(name).cloned() {
            let substitutions = infer_type_arguments(&generic, &argument_types)?;
            self.check_requirements(&generic, &substitutions, &[])?;
            let type_arguments = generic
                .type_parameters
                .iter()
                .map(|parameter| substitutions[parameter].clone())
                .collect::<Vec<_>>();
            let return_type = substitute(&generic.return_type, &substitutions);
            let function = self.specialize(generic, type_arguments, substitutions)?;
            return Ok(typed(
                return_type,
                ExpressionKind::Call {
                    function,
                    arguments,
                },
            ));
        }

        let candidates: Vec<_> = self
            .impl_methods
            .iter()
            .filter(|method| {
                method.signature.name == name
                    && method
                        .definition
                        .parameters
                        .iter()
                        .map(|parameter| &parameter.ty)
                        .eq(argument_types.iter())
            })
            .cloned()
            .collect();
        match candidates.as_slice() {
            [method] => Ok(typed(
                method.definition.return_type.clone(),
                ExpressionKind::Call {
                    function: method.function,
                    arguments,
                },
            )),
            [] => Err(Error::new(format!(
                "no function or trait method `{name}` accepts the supplied argument types"
            ))),
            _ => Err(Error::new(format!(
                "call to trait method `{name}` is ambiguous"
            ))),
        }
    }

    fn specialize(
        &mut self,
        definition: FunctionDefinition,
        type_arguments: Vec<Type>,
        substitutions: HashMap<String, Type>,
    ) -> Result<usize, Error> {
        let key = (definition.name.clone(), type_arguments.clone());
        if let Some(function) = self.specializations.get(&key) {
            return Ok(*function);
        }
        let parameters = definition
            .parameters
            .iter()
            .map(|parameter| substitute(&parameter.ty, &substitutions))
            .collect();
        let return_type = substitute(&definition.return_type, &substitutions);
        let suffix = type_arguments
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(".");
        let function = self.reserve_function(
            format!("kal.fn.{}.{}", definition.name, suffix),
            parameters,
            return_type,
        );
        self.specializations.insert(key, function);
        self.lower_definition(&definition, function, &substitutions)?;
        Ok(function)
    }

    fn check_requirements(
        &self,
        definition: &FunctionDefinition,
        substitutions: &HashMap<String, Type>,
        caller_constraints: &[Constraint],
    ) -> Result<(), Error> {
        for constraint in &definition.constraints {
            let ty = substitutions.get(&constraint.parameter).ok_or_else(|| {
                Error::new(format!(
                    "could not determine type parameter `{}` for `{}`",
                    constraint.parameter, definition.name
                ))
            })?;
            let satisfied = match ty {
                Type::Parameter(parameter) => caller_constraints.iter().any(|caller| {
                    caller.parameter == *parameter && caller.trait_name == constraint.trait_name
                }),
                concrete => self.has_impl(&constraint.trait_name, concrete),
            };
            if !satisfied {
                return Err(Error::new(format!(
                    "type `{ty}` does not implement trait `{}` required by `{}`",
                    constraint.trait_name, definition.name
                )));
            }
        }
        Ok(())
    }

    fn has_impl(&self, trait_name: &str, ty: &Type) -> bool {
        self.impl_methods
            .iter()
            .any(|method| method.trait_name == trait_name && method.for_type == *ty)
    }

    fn reserve_function(
        &mut self,
        symbol: String,
        parameters: Vec<Type>,
        return_type: Type,
    ) -> usize {
        let index = self.pending_functions.len();
        self.pending_functions.push(PendingFunction {
            symbol,
            parameters,
            return_type,
            body: None,
        });
        index
    }
}

fn parse_declarations(expressions: Vec<Expression>) -> Result<Declarations, Error> {
    let mut declarations = Declarations::default();
    for expression in expressions {
        let form = list(&expression, "top-level form must be a list")?;
        match first_symbol(form, "top-level form must begin with a declaration")? {
            "trait" => declarations.traits.push(parse_trait(form)?),
            "impl" => declarations.implementations.push(parse_impl(form)?),
            "defn" => declarations
                .functions
                .push(parse_function(form, "defn", true)?),
            other => return Err(Error::new(format!("unsupported top-level form `{other}`"))),
        }
    }
    Ok(declarations)
}

fn parse_trait(form: &[Expression]) -> Result<TraitDefinition, Error> {
    let name = symbol(form.get(1), "trait is missing its name")?.to_owned();
    if form.len() < 3 {
        return Err(Error::new(format!(
            "trait `{name}` must declare at least one method"
        )));
    }
    let mut method_names = HashSet::new();
    let mut methods = Vec::new();
    for method in &form[2..] {
        let method = list(method, "trait method must be a list")?;
        if method.len() != 5 {
            return Err(Error::new("trait method must contain only a signature"));
        }
        expect_symbol(method.first(), "fn", "trait method must start with `fn`")?;
        let name = symbol(method.get(1), "trait method is missing its name")?.to_owned();
        if !method_names.insert(name.clone()) {
            return Err(Error::new(format!(
                "method `{name}` is declared more than once in trait"
            )));
        }
        let parameters = parse_parameters(method.get(2), &[], true)?;
        expect_symbol(method.get(3), "->", "expected `->` before the return type")?;
        let return_type = parse_type(
            method
                .get(4)
                .ok_or_else(|| Error::new("trait method is missing a return type"))?,
            &[],
            true,
        )?;
        methods.push(MethodSignature {
            name,
            parameters,
            return_type,
        });
    }
    Ok(TraitDefinition { name, methods })
}

fn parse_impl(form: &[Expression]) -> Result<ImplDefinition, Error> {
    let trait_name = symbol(form.get(1), "implementation is missing its trait")?.to_owned();
    expect_symbol(form.get(2), "for", "expected `for` in trait implementation")?;
    let for_type = parse_type(
        form.get(3)
            .ok_or_else(|| Error::new("implementation is missing its type"))?,
        &[],
        false,
    )?;
    if form.len() < 5 {
        return Err(Error::new("trait implementation must define its methods"));
    }
    let methods = form[4..]
        .iter()
        .map(|method| {
            parse_function(
                list(method, "implemented method must be a list")?,
                "fn",
                false,
            )
        })
        .collect::<Result<_, _>>()?;
    Ok(ImplDefinition {
        trait_name,
        for_type,
        methods,
    })
}

fn parse_function(
    form: &[Expression],
    keyword: &str,
    allow_generics: bool,
) -> Result<FunctionDefinition, Error> {
    expect_symbol(
        form.first(),
        keyword,
        &format!("function must start with `{keyword}`"),
    )?;
    let name = symbol(form.get(1), "function is missing its name")?.to_owned();
    let has_generics = allow_generics && form.len() == 7;
    let expected_len = if has_generics { 7 } else { 6 };
    if form.len() != expected_len {
        return Err(Error::new(format!(
            "function `{name}` must contain exactly one body expression"
        )));
    }
    let (type_parameters, constraints, parameter_index) = if has_generics {
        let (parameters, constraints) = parse_generic_parameters(&form[2])?;
        (parameters, constraints, 3)
    } else {
        (Vec::new(), Vec::new(), 2)
    };
    let parameters = parse_parameters(form.get(parameter_index), &type_parameters, false)?;
    expect_symbol(
        form.get(parameter_index + 1),
        "->",
        "expected `->` before the return type",
    )?;
    let return_type = parse_type(
        form.get(parameter_index + 2)
            .ok_or_else(|| Error::new(format!("function `{name}` is missing a return type")))?,
        &type_parameters,
        false,
    )?;
    let body = form[parameter_index + 3].clone();
    Ok(FunctionDefinition {
        name,
        type_parameters,
        constraints,
        parameters,
        return_type,
        body,
    })
}

fn parse_generic_parameters(
    expression: &Expression,
) -> Result<(Vec<String>, Vec<Constraint>), Error> {
    let elements = list(expression, "generic parameters must be enclosed in `[]`")?;
    let where_index = elements
        .iter()
        .position(|element| matches!(element, Expression::Symbol(name) if name == "where"))
        .ok_or_else(|| Error::new("generic parameters require a `where` clause"))?;
    if where_index == 0 || where_index + 1 == elements.len() {
        return Err(Error::new(
            "generic parameters require names and at least one trait requirement",
        ));
    }
    let mut seen = HashSet::new();
    let mut parameters = Vec::new();
    for parameter in &elements[..where_index] {
        let parameter = symbol(Some(parameter), "generic parameter must be a name")?.to_owned();
        if !seen.insert(parameter.clone()) {
            return Err(Error::new(format!(
                "generic parameter `{parameter}` is declared more than once"
            )));
        }
        parameters.push(parameter);
    }
    let mut constraints = Vec::new();
    for constraint in &elements[where_index + 1..] {
        let constraint = list(constraint, "trait requirement must be a list")?;
        if constraint.len() != 3 {
            return Err(Error::new(
                "trait requirement must be `(implements TypeParameter Trait)`",
            ));
        }
        expect_symbol(
            constraint.first(),
            "implements",
            "trait requirement must start with `implements`",
        )?;
        let parameter = symbol(
            constraint.get(1),
            "trait requirement is missing a type parameter",
        )?;
        if !parameters.iter().any(|candidate| candidate == parameter) {
            return Err(Error::new(format!(
                "trait requirement references unknown type parameter `{parameter}`"
            )));
        }
        let trait_name = symbol(constraint.get(2), "trait requirement is missing a trait")?;
        constraints.push(Constraint {
            parameter: parameter.to_owned(),
            trait_name: trait_name.to_owned(),
        });
    }
    Ok((parameters, constraints))
}

fn parse_parameters(
    expression: Option<&Expression>,
    type_parameters: &[String],
    allow_self: bool,
) -> Result<Vec<Parameter>, Error> {
    let parameters = list(
        expression.ok_or_else(|| Error::new("function is missing its parameter list"))?,
        "function parameters must be a list",
    )?;
    let mut names = HashSet::new();
    parameters
        .iter()
        .map(|parameter| {
            let parameter = list(parameter, "function parameter must be `(name type)`")?;
            if parameter.len() != 2 {
                return Err(Error::new("function parameter must be `(name type)`"));
            }
            let name = symbol(parameter.first(), "function parameter must have a name")?.to_owned();
            if !names.insert(name.clone()) {
                return Err(Error::new(format!(
                    "function parameter `{name}` is declared more than once"
                )));
            }
            Ok(Parameter {
                name,
                ty: parse_type(&parameter[1], type_parameters, allow_self)?,
            })
        })
        .collect()
}

fn parse_type(
    expression: &Expression,
    type_parameters: &[String],
    allow_self: bool,
) -> Result<Type, Error> {
    match expression {
        Expression::Symbol(name) if name == "Int" => Ok(Type::Int),
        Expression::Symbol(name) if name == "String" => Ok(Type::String),
        Expression::Symbol(name) if name == "Unit" => Ok(Type::Unit),
        Expression::Symbol(name) if allow_self && name == "Self" => Ok(Type::TraitSelf),
        Expression::Symbol(name) if type_parameters.iter().any(|parameter| parameter == name) => {
            Ok(Type::Parameter(name.clone()))
        }
        Expression::List(elements)
            if elements.len() == 2
                && matches!(&elements[0], Expression::Symbol(name) if name == "Array") =>
        {
            Ok(Type::Array(Box::new(parse_type(
                &elements[1],
                type_parameters,
                allow_self,
            )?)))
        }
        _ => Err(Error::new("unknown type")),
    }
}

fn check_main_signature(definition: &FunctionDefinition) -> Result<(), Error> {
    if !definition.type_parameters.is_empty() {
        return Err(Error::new("main cannot be generic"));
    }
    if definition.parameters.len() != 1
        || definition.parameters[0].ty != Type::Array(Box::new(Type::String))
    {
        return Err(Error::new(
            "main must accept exactly one `Array String` parameter",
        ));
    }
    if definition.return_type != Type::Int {
        return Err(Error::new("main must return `Int`"));
    }
    Ok(())
}

fn infer_type_arguments(
    function: &FunctionDefinition,
    arguments: &[Type],
) -> Result<HashMap<String, Type>, Error> {
    if function.parameters.len() != arguments.len() {
        return Err(Error::new(format!(
            "function `{}` expects {} arguments, received {}",
            function.name,
            function.parameters.len(),
            arguments.len()
        )));
    }
    let mut substitutions = HashMap::new();
    for (parameter, argument) in function.parameters.iter().zip(arguments) {
        unify(&parameter.ty, argument, &mut substitutions).map_err(|_| {
            Error::new(format!(
                "arguments to `{}` do not match its parameter types",
                function.name
            ))
        })?;
    }
    for parameter in &function.type_parameters {
        if !substitutions.contains_key(parameter) {
            return Err(Error::new(format!(
                "could not determine type parameter `{parameter}` for `{}`",
                function.name
            )));
        }
    }
    Ok(substitutions)
}

fn unify(
    expected: &Type,
    actual: &Type,
    substitutions: &mut HashMap<String, Type>,
) -> Result<(), ()> {
    match expected {
        Type::Parameter(name) => match substitutions.get(name) {
            Some(previous) if previous != actual => Err(()),
            Some(_) => Ok(()),
            None => {
                substitutions.insert(name.clone(), actual.clone());
                Ok(())
            }
        },
        Type::Array(expected) => match actual {
            Type::Array(actual) => unify(expected, actual, substitutions),
            _ => Err(()),
        },
        _ if expected == actual => Ok(()),
        _ => Err(()),
    }
}

fn substitute(ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
    match ty {
        Type::Parameter(name) => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| ty.clone()),
        Type::Array(element) => Type::Array(Box::new(substitute(element, substitutions))),
        _ => ty.clone(),
    }
}

fn replace_self(ty: &Type, replacement: &Type) -> Type {
    match ty {
        Type::TraitSelf => replacement.clone(),
        Type::Array(element) => Type::Array(Box::new(replace_self(element, replacement))),
        _ => ty.clone(),
    }
}

fn match_trait_signature(signature: &MethodSignature, arguments: &[Type]) -> Option<Type> {
    if signature.parameters.len() != arguments.len() {
        return None;
    }
    let mut self_type = None;
    for (parameter, argument) in signature.parameters.iter().zip(arguments) {
        if !match_self_type(&parameter.ty, argument, &mut self_type) {
            return None;
        }
    }
    self_type
}

fn match_self_type(pattern: &Type, actual: &Type, self_type: &mut Option<Type>) -> bool {
    match pattern {
        Type::TraitSelf => match self_type {
            Some(previous) => previous == actual,
            None => {
                *self_type = Some(actual.clone());
                true
            }
        },
        Type::Array(pattern) => match actual {
            Type::Array(actual) => match_self_type(pattern, actual, self_type),
            _ => false,
        },
        _ => pattern == actual,
    }
}

fn check_arguments(name: &str, actual: &[Type], expected: &[Type]) -> Result<(), Error> {
    if actual == expected {
        return Ok(());
    }
    Err(Error::new(format!(
        "arguments to `{name}` do not match its parameter types"
    )))
}

fn typed(ty: Type, kind: ExpressionKind) -> TypedExpression {
    TypedExpression { ty, kind }
}

fn list<'a>(expression: &'a Expression, message: &str) -> Result<&'a [Expression], Error> {
    match expression {
        Expression::List(expressions) => Ok(expressions),
        _ => Err(Error::new(message)),
    }
}

fn first_symbol<'a>(expressions: &'a [Expression], message: &str) -> Result<&'a str, Error> {
    symbol(expressions.first(), message)
}

fn symbol<'a>(expression: Option<&'a Expression>, message: &str) -> Result<&'a str, Error> {
    match expression {
        Some(Expression::Symbol(symbol)) => Ok(symbol),
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

use cranelift_codegen::ir::{AbiParam, InstBuilder, TrapCode, Value, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::Error;
use crate::semantics::{ExpressionKind, Program, Type, TypedExpression};

pub(crate) fn emit_object(program: &Program) -> Result<Vec<u8>, Error> {
    let mut flags = settings::builder();
    flags
        .set("is_pic", "true")
        .map_err(|error| Error::new(format!("could not configure Cranelift: {error}")))?;
    let isa = cranelift_native::builder()
        .map_err(|error| Error::new(format!("unsupported compilation host: {error}")))?
        .finish(settings::Flags::new(flags))
        .map_err(|error| Error::new(format!("could not create the native backend: {error}")))?;
    let builder = ObjectBuilder::new(isa, "kal-program", default_libcall_names())
        .map_err(|error| Error::new(format!("could not create an object module: {error}")))?;
    let mut module = ObjectModule::new(builder);
    let frontend_config = module.target_config();
    let pointer_type = frontend_config.pointer_type();

    let mut write_signature = module.make_signature();
    write_signature.params.push(AbiParam::new(types::I32));
    write_signature.params.push(AbiParam::new(pointer_type));
    write_signature.params.push(AbiParam::new(pointer_type));
    write_signature.returns.push(AbiParam::new(pointer_type));
    let write_id = module
        .declare_function("write", Linkage::Import, &write_signature)
        .map_err(|error| Error::new(format!("could not declare `write`: {error}")))?;

    let mut data_ids = Vec::with_capacity(program.strings.len());
    for (index, bytes) in program.strings.iter().enumerate() {
        let data_id = module
            .declare_data(&format!("kal.string.{index}"), Linkage::Local, false, false)
            .map_err(|error| Error::new(format!("could not declare string data: {error}")))?;
        let mut description = DataDescription::new();
        description.define(bytes.clone().into_boxed_slice());
        module
            .define_data(data_id, &description)
            .map_err(|error| Error::new(format!("could not define string data: {error}")))?;
        data_ids.push((data_id, bytes.len()));
    }

    let mut signatures = Vec::with_capacity(program.functions.len());
    let mut function_ids = Vec::with_capacity(program.functions.len());
    for (index, function) in program.functions.iter().enumerate() {
        let mut signature = module.make_signature();
        if index == program.main {
            signature.params.push(AbiParam::new(types::I32));
            signature.params.push(AbiParam::new(pointer_type));
            signature.returns.push(AbiParam::new(types::I32));
        } else {
            for parameter in &function.parameters {
                signature
                    .params
                    .push(AbiParam::new(codegen_type(parameter, pointer_type)?));
            }
            if function.return_type != Type::Unit {
                signature.returns.push(AbiParam::new(codegen_type(
                    &function.return_type,
                    pointer_type,
                )?));
            }
        }
        let function_id = module
            .declare_function(
                &function.symbol,
                if index == program.main {
                    Linkage::Export
                } else {
                    Linkage::Local
                },
                &signature,
            )
            .map_err(|error| {
                Error::new(format!(
                    "could not declare function `{}`: {error}",
                    function.symbol
                ))
            })?;
        signatures.push(signature);
        function_ids.push(function_id);
    }

    for (index, program_function) in program.functions.iter().enumerate() {
        let mut context = module.make_context();
        context.func.signature = signatures[index].clone();
        let mut function_context = FunctionBuilderContext::new();
        {
            let mut function = FunctionBuilder::new(&mut context.func, &mut function_context);
            let entry = function.create_block();
            function.append_block_params_for_function_params(entry);
            function.switch_to_block(entry);
            function.seal_block(entry);

            let parameters = if index == program.main {
                Vec::new()
            } else {
                function.block_params(entry).to_vec()
            };
            let value = emit_expression(
                &program_function.body,
                &parameters,
                &mut function,
                &mut module,
                &function_ids,
                write_id,
                &data_ids,
                pointer_type,
            )?;

            if index == program.main {
                let exit_code = require_value(value, "main body")?;
                let exit_code = function.ins().ireduce(types::I32, exit_code);
                function.ins().return_(&[exit_code]);
            } else if program_function.return_type == Type::Unit {
                function.ins().return_(&[]);
            } else {
                let value = require_value(
                    value,
                    &format!("function `{}` body", program_function.symbol),
                )?;
                function.ins().return_(&[value]);
            }
            function.finalize(frontend_config);
        }

        module
            .define_function(function_ids[index], &mut context)
            .map_err(|error| {
                Error::new(format!(
                    "could not compile function `{}`: {error}",
                    program_function.symbol
                ))
            })?;
        module.clear_context(&mut context);
    }

    module
        .finish()
        .emit()
        .map_err(|error| Error::new(format!("could not emit the object file: {error}")))
}

#[allow(clippy::too_many_arguments)]
fn emit_expression(
    expression: &TypedExpression,
    parameters: &[Value],
    function: &mut FunctionBuilder<'_>,
    module: &mut ObjectModule,
    function_ids: &[FuncId],
    write_id: FuncId,
    data_ids: &[(DataId, usize)],
    pointer_type: cranelift_codegen::ir::Type,
) -> Result<Option<Value>, Error> {
    match &expression.kind {
        ExpressionKind::Integer(value) => Ok(Some(function.ins().iconst(types::I64, *value))),
        ExpressionKind::String => Ok(None),
        ExpressionKind::Parameter(index) => parameters
            .get(*index)
            .copied()
            .map(Some)
            .ok_or_else(|| Error::new("using the native `args` value is not implemented yet")),
        ExpressionKind::Do(expressions) => {
            let mut value = None;
            for expression in expressions {
                value = emit_expression(
                    expression,
                    parameters,
                    function,
                    module,
                    function_ids,
                    write_id,
                    data_ids,
                    pointer_type,
                )?;
            }
            Ok(value)
        }
        ExpressionKind::Print(index) => {
            let (data_id, length) = data_ids[*index];
            let write = module.declare_func_in_func(write_id, function.func);
            let data = module.declare_data_in_func(data_id, function.func);
            let address = function.ins().symbol_value(pointer_type, data);
            let stdout = function.ins().iconst(types::I32, 1);
            let length = function.ins().iconst(pointer_type, length as i64);
            function.ins().call(write, &[stdout, address, length]);
            Ok(None)
        }
        ExpressionKind::Add(left, right) => {
            let left = require_value(
                emit_expression(
                    left,
                    parameters,
                    function,
                    module,
                    function_ids,
                    write_id,
                    data_ids,
                    pointer_type,
                )?,
                "left operand of `+`",
            )?;
            let right = require_value(
                emit_expression(
                    right,
                    parameters,
                    function,
                    module,
                    function_ids,
                    write_id,
                    data_ids,
                    pointer_type,
                )?,
                "right operand of `+`",
            )?;
            let (sum, overflow) = function.ins().sadd_overflow(left, right);
            function.ins().trapnz(overflow, TrapCode::INTEGER_OVERFLOW);
            Ok(Some(sum))
        }
        ExpressionKind::Call {
            function: callee,
            arguments,
        } => {
            let mut values = Vec::with_capacity(arguments.len());
            for argument in arguments {
                let value = emit_expression(
                    argument,
                    parameters,
                    function,
                    module,
                    function_ids,
                    write_id,
                    data_ids,
                    pointer_type,
                )?;
                values.push(require_value(value, "function argument")?);
            }
            let callee = module.declare_func_in_func(function_ids[*callee], function.func);
            let call = function.ins().call(callee, &values);
            if expression.ty == Type::Unit {
                Ok(None)
            } else {
                Ok(Some(function.inst_results(call)[0]))
            }
        }
    }
}

fn codegen_type(
    ty: &Type,
    pointer_type: cranelift_codegen::ir::Type,
) -> Result<cranelift_codegen::ir::Type, Error> {
    match ty {
        Type::Int => Ok(types::I64),
        Type::String | Type::Array(_) => Ok(pointer_type),
        Type::Unit => Err(Error::new("`Unit` cannot be used as a function parameter")),
        Type::Parameter(_) | Type::TraitSelf => Err(Error::new(format!(
            "unresolved type `{ty}` reached code generation"
        ))),
    }
}

fn require_value(value: Option<Value>, description: &str) -> Result<Value, Error> {
    value.ok_or_else(|| Error::new(format!("{description} does not produce a native value")))
}

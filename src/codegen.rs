use cranelift_codegen::ir::{AbiParam, InstBuilder, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::Error;
use crate::semantics::Program;

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

    let mut data_ids = Vec::with_capacity(program.output.len());
    for (index, bytes) in program.output.iter().enumerate() {
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

    let mut main_signature = module.make_signature();
    main_signature.params.push(AbiParam::new(types::I32));
    main_signature.params.push(AbiParam::new(pointer_type));
    main_signature.returns.push(AbiParam::new(types::I32));
    let main_id = module
        .declare_function("main", Linkage::Export, &main_signature)
        .map_err(|error| Error::new(format!("could not declare `main`: {error}")))?;

    let mut context = module.make_context();
    context.func.signature = main_signature;
    let mut function_context = FunctionBuilderContext::new();
    {
        let mut function = FunctionBuilder::new(&mut context.func, &mut function_context);
        let entry = function.create_block();
        function.append_block_params_for_function_params(entry);
        function.switch_to_block(entry);
        function.seal_block(entry);

        let write = module.declare_func_in_func(write_id, function.func);
        for (data_id, length) in data_ids {
            let data = module.declare_data_in_func(data_id, function.func);
            let address = function.ins().symbol_value(pointer_type, data);
            let stdout = function.ins().iconst(types::I32, 1);
            let length = function.ins().iconst(pointer_type, length as i64);
            function.ins().call(write, &[stdout, address, length]);
        }

        let exit_code = function.ins().iconst(types::I32, program.exit_code);
        function.ins().return_(&[exit_code]);
        function.finalize(frontend_config);
    }

    module
        .define_function(main_id, &mut context)
        .map_err(|error| Error::new(format!("could not compile `main`: {error}")))?;
    module.clear_context(&mut context);

    module
        .finish()
        .emit()
        .map_err(|error| Error::new(format!("could not emit the object file: {error}")))
}

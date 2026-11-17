//! Stack-overflow reporting for direct-native binaries (#1784).
//!
//! Unbounded recursion used to die with a raw SIGSEGV and no message. On
//! Linux x86-64 the generated `main` now installs a SIGSEGV handler on an
//! alternate signal stack. When the faulting address lies in the main
//! thread's stack region, the handler prints the same structured runtime
//! line as other runtime errors, `{"kind":"runtime","message":"stack
//! overflow"}`, and exits with status 1. Any other SIGSEGV, including one
//! sent with kill(), restores the default action and re-raises the signal,
//! so the process still dies by SIGSEGV exactly as it did before.
//!
//! Other hosts install nothing yet; their `struct sigaction` and
//! `stack_t` layouts differ and are not covered by tests here.

use crate::CraneliftBackendError;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    AbiParam, InstBuilder, MemFlags, StackSlotData, StackSlotKind, Type, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::ObjectModule;

const MESSAGE: &[u8] = b"{\"kind\":\"runtime\",\"message\":\"stack overflow\"}\n";
const ALT_STACK_BYTES: i64 = 64 * 1024;
/// Faults this far below the stack limit still count as stack overflow:
/// a large frame can step past the guard page before it touches memory.
const GUARD_SLACK_BYTES: i64 = 1024 * 1024;
/// Never classify the low address range as stack: those are null-pointer
/// style faults, which keep their default SIGSEGV behaviour.
const LOW_ADDRESS_FLOOR: i64 = 64 * 1024;

// Linux x86-64 (glibc) ABI constants and layouts.
const SIGSEGV: i64 = 11;
const RLIMIT_STACK: i64 = 3;
const SA_SIGINFO: i64 = 0x4;
const SA_ONSTACK: i64 = 0x0800_0000;
const SIGACTION_BYTES: u32 = 152;
const SIGACTION_FLAGS_OFFSET: i32 = 136;
const STACK_T_BYTES: u32 = 24;
const STACK_T_SIZE_OFFSET: i32 = 16;
const SIGINFO_CODE_OFFSET: i32 = 8;
const SIGINFO_ADDR_OFFSET: i32 = 16;

/// Module items the guard needs. `None` when the host is not covered.
pub(crate) struct StackOverflowGuard {
    handler: FuncId,
    sigaltstack: FuncId,
    sigaction: FuncId,
    getrlimit: FuncId,
    alt_stack: DataId,
    stack_t: DataId,
    action: DataId,
    bounds: DataId,
}

fn backend_error(context: &str, message: impl std::fmt::Display) -> CraneliftBackendError {
    CraneliftBackendError::new(format!("stack overflow guard {context}: {message}"))
}

fn zeroed_data(
    module: &mut ObjectModule,
    name: &str,
    bytes: u32,
) -> Result<DataId, CraneliftBackendError> {
    let id = module
        .declare_data(name, Linkage::Local, true, false)
        .map_err(|message| backend_error("declare data", message))?;
    let mut description = DataDescription::new();
    description.define_zeroinit(bytes as usize);
    description.set_align(16);
    module
        .define_data(id, &description)
        .map_err(|message| backend_error("define data", message))?;
    Ok(id)
}

fn import(
    module: &mut ObjectModule,
    name: &str,
    params: &[Type],
    returns: &[Type],
) -> Result<FuncId, CraneliftBackendError> {
    let mut signature = module.make_signature();
    signature
        .params
        .extend(params.iter().copied().map(AbiParam::new));
    signature
        .returns
        .extend(returns.iter().copied().map(AbiParam::new));
    module
        .declare_function(name, Linkage::Import, &signature)
        .map_err(|message| backend_error(&format!("declare {name}"), message))
}

/// Declares and defines the handler and its data. Call before `main`'s
/// function context is created.
pub(crate) fn declare(
    module: &mut ObjectModule,
) -> Result<Option<StackOverflowGuard>, CraneliftBackendError> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Ok(None);
    }
    let ptr = module.target_config().pointer_type();
    // Signatures match the existing `write` and `_exit` imports in lib.rs,
    // so these declarations resolve to the same import symbols.
    let write = import(module, "write", &[types::I32, ptr, ptr], &[ptr])?;
    let exit = import(module, "_exit", &[types::I32], &[types::I32])?;
    let sigaltstack = import(module, "sigaltstack", &[ptr, ptr], &[types::I32])?;
    let sigaction = import(module, "sigaction", &[types::I32, ptr, ptr], &[types::I32])?;
    let getrlimit = import(module, "getrlimit", &[types::I32, ptr], &[types::I32])?;
    let raise = import(module, "raise", &[types::I32], &[types::I32])?;

    let alt_stack = zeroed_data(
        module,
        "__axiom_stack_overflow_alt_stack",
        ALT_STACK_BYTES as u32,
    )?;
    let stack_t = zeroed_data(module, "__axiom_stack_overflow_stack_t", STACK_T_BYTES)?;
    let action = zeroed_data(module, "__axiom_stack_overflow_sigaction", SIGACTION_BYTES)?;
    // [low, high) bounds of the main thread's stack region, as u64 words.
    let bounds = zeroed_data(module, "__axiom_stack_overflow_bounds", 16)?;
    let message = {
        let id = module
            .declare_data(
                "__axiom_stack_overflow_message",
                Linkage::Local,
                false,
                false,
            )
            .map_err(|message| backend_error("declare message", message))?;
        let mut description = DataDescription::new();
        description.define(MESSAGE.to_vec().into_boxed_slice());
        module
            .define_data(id, &description)
            .map_err(|message| backend_error("define message", message))?;
        id
    };

    let mut context = module.make_context();
    for param in [types::I32, ptr, ptr] {
        context.func.signature.params.push(AbiParam::new(param));
    }
    let handler = module
        .declare_function(
            "__axiom_stack_overflow_handler",
            Linkage::Local,
            &context.func.signature,
        )
        .map_err(|message| backend_error("declare handler", message))?;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let entry = builder.create_block();
        let report = builder.create_block();
        let fallback = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let params = builder.block_params(entry).to_vec();
        let (signal, info) = (params[0], params[1]);

        let bounds_gv = module.declare_data_in_func(bounds, builder.func);
        let bounds_addr = builder.ins().symbol_value(ptr, bounds_gv);
        let fault = builder
            .ins()
            .load(ptr, MemFlags::trusted(), info, SIGINFO_ADDR_OFFSET);
        let low = builder.ins().load(ptr, MemFlags::trusted(), bounds_addr, 0);
        let high = builder.ins().load(ptr, MemFlags::trusted(), bounds_addr, 8);
        let at_or_above_low = builder
            .ins()
            .icmp(IntCC::UnsignedGreaterThanOrEqual, fault, low);
        let below_high = builder.ins().icmp(IntCC::UnsignedLessThan, fault, high);
        // si_code > 0 means the kernel raised the fault (SEGV_MAPERR,
        // SEGV_ACCERR, ...). A SIGSEGV sent with kill() has si_code <= 0 and
        // carries a sender pid where si_addr would be.
        let code = builder
            .ins()
            .load(types::I32, MemFlags::trusted(), info, SIGINFO_CODE_OFFSET);
        let kernel_fault = builder.ins().icmp_imm(IntCC::SignedGreaterThan, code, 0);
        let in_range = builder.ins().band(at_or_above_low, below_high);
        let in_stack = builder.ins().band(in_range, kernel_fault);
        builder.ins().brif(in_stack, report, &[], fallback, &[]);

        builder.switch_to_block(report);
        builder.seal_block(report);
        let write_ref = module.declare_func_in_func(write, builder.func);
        let exit_ref = module.declare_func_in_func(exit, builder.func);
        let message_gv = module.declare_data_in_func(message, builder.func);
        let message_addr = builder.ins().symbol_value(ptr, message_gv);
        let stderr = builder.ins().iconst(types::I32, 2);
        let length = builder.ins().iconst(ptr, MESSAGE.len() as i64);
        builder
            .ins()
            .call(write_ref, &[stderr, message_addr, length]);
        let status = builder.ins().iconst(types::I32, 1);
        builder.ins().call(exit_ref, &[status]);
        builder.ins().return_(&[]);

        // Not a stack fault: reinstate SIG_DFL and re-raise. The signal stays
        // blocked until the handler returns, then the default action ends the
        // process by SIGSEGV, as before. Re-raising also covers a SIGSEGV sent
        // with kill(), where returning alone would just interrupt a syscall.
        builder.switch_to_block(fallback);
        builder.seal_block(fallback);
        let sigaction_ref = module.declare_func_in_func(sigaction, builder.func);
        let action_gv = module.declare_data_in_func(action, builder.func);
        let action_addr = builder.ins().symbol_value(ptr, action_gv);
        let zero_ptr = builder.ins().iconst(ptr, 0);
        let zero_i32 = builder.ins().iconst(types::I32, 0);
        builder
            .ins()
            .store(MemFlags::trusted(), zero_ptr, action_addr, 0);
        builder.ins().store(
            MemFlags::trusted(),
            zero_i32,
            action_addr,
            SIGACTION_FLAGS_OFFSET,
        );
        builder
            .ins()
            .call(sigaction_ref, &[signal, action_addr, zero_ptr]);
        let raise_ref = module.declare_func_in_func(raise, builder.func);
        builder.ins().call(raise_ref, &[signal]);
        builder.ins().return_(&[]);
        builder.finalize();
    }
    module
        .define_function(handler, &mut context)
        .map_err(|message| backend_error("define handler", message))?;
    module.clear_context(&mut context);

    Ok(Some(StackOverflowGuard {
        handler,
        sigaltstack,
        sigaction,
        getrlimit,
        alt_stack,
        stack_t,
        action,
        bounds,
    }))
}

/// Emits the handler installation at the top of `main`'s entry block.
pub(crate) fn install(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    guard: &StackOverflowGuard,
) {
    let ptr = module.target_config().pointer_type();
    let flags = MemFlags::trusted();

    // High bound: an address in main's own frame. Recursion grows below it.
    let marker =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 4));
    let high = builder.ins().stack_addr(ptr, marker, 0);
    let getrlimit_ref = module.declare_func_in_func(guard.getrlimit, builder.func);
    let zero = builder.ins().iconst(ptr, 0);
    builder.ins().store(flags, zero, high, 0);
    let resource = builder.ins().iconst(types::I32, RLIMIT_STACK);
    builder.ins().call(getrlimit_ref, &[resource, high]);
    // rlim_cur; RLIM_INFINITY and failures leave a value >= high or 0.
    let limit = builder.ins().load(ptr, flags, high, 0);
    let limit_covers_all = builder
        .ins()
        .icmp(IntCC::UnsignedGreaterThanOrEqual, limit, high);
    let limit_low = builder.ins().isub(high, limit);
    let raw_low = builder.ins().select(limit_covers_all, zero, limit_low);
    let slack = builder.ins().iconst(ptr, GUARD_SLACK_BYTES);
    let floor = builder.ins().iconst(ptr, LOW_ADDRESS_FLOOR);
    let floor_plus_slack = builder
        .ins()
        .iconst(ptr, LOW_ADDRESS_FLOOR + GUARD_SLACK_BYTES);
    let too_low = builder
        .ins()
        .icmp(IntCC::UnsignedLessThan, raw_low, floor_plus_slack);
    let slack_low = builder.ins().isub(raw_low, slack);
    let low = builder.ins().select(too_low, floor, slack_low);

    let bounds_gv = module.declare_data_in_func(guard.bounds, builder.func);
    let bounds_addr = builder.ins().symbol_value(ptr, bounds_gv);
    builder.ins().store(flags, low, bounds_addr, 0);
    builder.ins().store(flags, high, bounds_addr, 8);

    let alt_gv = module.declare_data_in_func(guard.alt_stack, builder.func);
    let alt_addr = builder.ins().symbol_value(ptr, alt_gv);
    let stack_t_gv = module.declare_data_in_func(guard.stack_t, builder.func);
    let stack_t_addr = builder.ins().symbol_value(ptr, stack_t_gv);
    let alt_size = builder.ins().iconst(ptr, ALT_STACK_BYTES);
    builder.ins().store(flags, alt_addr, stack_t_addr, 0);
    builder
        .ins()
        .store(flags, alt_size, stack_t_addr, STACK_T_SIZE_OFFSET);
    let sigaltstack_ref = module.declare_func_in_func(guard.sigaltstack, builder.func);
    builder.ins().call(sigaltstack_ref, &[stack_t_addr, zero]);

    let handler_ref = module.declare_func_in_func(guard.handler, builder.func);
    let handler_addr = builder.ins().func_addr(ptr, handler_ref);
    let action_gv = module.declare_data_in_func(guard.action, builder.func);
    let action_addr = builder.ins().symbol_value(ptr, action_gv);
    let action_flags = builder.ins().iconst(types::I32, SA_SIGINFO | SA_ONSTACK);
    builder.ins().store(flags, handler_addr, action_addr, 0);
    builder
        .ins()
        .store(flags, action_flags, action_addr, SIGACTION_FLAGS_OFFSET);
    let sigaction_ref = module.declare_func_in_func(guard.sigaction, builder.func);
    let signal = builder.ins().iconst(types::I32, SIGSEGV);
    builder
        .ins()
        .call(sigaction_ref, &[signal, action_addr, zero]);
}

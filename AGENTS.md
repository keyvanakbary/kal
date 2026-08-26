# Kal repository instructions

These instructions apply to the entire repository.

## Sources of truth

- `README.md` is the source of truth for Kal's user-visible syntax, semantics,
  target, current capabilities, and roadmap.
- This file defines the engineering workflow for changing the repository.
- Do not present planned README behavior as implemented behavior.
- If a change alters a language decision, update the implementation, tests, and
  README together.
- If intended behavior is not established by the README or existing tests, ask
  before inventing a new semantic rule.

## Required development cycle

Every behavior change and bug fix must follow red-green-refactor:

1. Add the smallest test that specifies one observable behavior.
2. Run it and confirm it fails for the intended reason.
3. Implement only enough production code to make that test pass.
4. Run the focused test and confirm it passes.
5. Refactor only after the behavior is green.
6. Run the complete quality gate before finishing.

Do not add production behavior before its failing test. A compiler bug starts
with a regression test that reproduces it.

## Test strategy

- Prefer black-box acceptance tests that write `.kal` source, invoke `kal build`,
  inspect the produced ELF when relevant, execute it, and assert status, stdout,
  and stderr.
- Add focused unit tests for parsing, name resolution, type checking, IR lowering,
  code generation, runtime behavior, and optimization when those layers acquire
  independent behavior.
- Failed compilation must not leave a new successful-looking executable or
  destroy a previously valid output.
- Development and optimized builds must preserve identical language semantics.
- When the JIT exists, run shared conformance programs through AOT and JIT paths.
- Keep benchmarks separate from correctness tests; do not add flaky wall-clock
  thresholds to the normal test suite.
- Add property or fuzz tests only when the relevant grammar or semantic boundary
  is stable enough to define useful invariants.

## Required quality gate

Run all of these commands before handing off a change:

```console
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Also compile and execute a representative `.kal` program when changing code
generation, linking, ABI behavior, or the runtime.

## Architecture guardrails

- The compiler is Rust using the toolchain pinned by `rust-toolchain.toml`.
- The current production backend is Cranelift.
- The current target is Linux amd64 using the System V ABI.
- The compilation path is source, syntax, semantics, typed Kal IR, Cranelift,
  object file, system linker, and native ELF.
- Keep frontend and typed IR behavior independent of AOT versus the future JIT.
- Keep generated primitives unboxed when their static types permit it.
- Resolve traits and generics statically and monomorphize them; do not introduce
  runtime dispatch without an explicit language decision.
- Keep integer-overflow behavior and other semantics consistent across build
  profiles.
- Keep unsafe Rust isolated behind small interfaces and document every safety
  invariant when unsafe code first becomes necessary.
- Produce output atomically: compile and link through temporary files, then
  publish the final executable only after success.

Do not replace Cranelift, add LLVM, introduce a hand-written amd64 production
backend, or broaden the target matrix without an explicit decision supported by
tests or benchmark evidence.

## Scope discipline

- Implement the current requested vertical slice, not unrelated future roadmap
  items.
- Do not add abstractions until at least one concrete test requires them; prefer
  extraction during refactoring over speculative framework design.
- Do not add type inference, gradual typing, implicit numeric promotion, runtime
  multiple dispatch, unrestricted mutation, macros, runtime `eval`, or
  self-hosting unless the language decision is deliberately changed first.
- Preserve the canonical `.kal` extension without rejecting an explicitly
  supplied source path solely because it has another extension.
- Add dependencies only when the current tested behavior needs them, and pin
  their resolved versions in `Cargo.lock`.
- Preserve unrelated user changes and generated artifacts outside the requested
  scope.

## Repository guide

- `src/syntax.rs`: tokenization and S-expression parsing
- `src/semantics.rs`: language validation and typed semantic lowering
- `src/codegen.rs`: Cranelift object generation
- `src/lib.rs`: build orchestration and atomic system linking
- `src/main.rs`: `kal` command-line interface
- `tests/`: black-box acceptance tests
- `examples/`: runnable `.kal` programs
- `README.md`: language design, user documentation, current status, and roadmap

Keep build products under `target/` or temporary directories. Do not commit
generated executables or temporary object files.

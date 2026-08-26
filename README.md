# Kal

Kal is an explicitly and statically typed Lisp designed to compile to fast native
executables. Its first target is Linux on amd64 using the System V ABI.

The project has three priorities:

1. Learn compiler construction deeply by keeping each compiler stage visible and
   well tested.
2. Grow into a useful general-purpose language for command-line programs.
3. Favor generated-program performance when it conflicts with optimized-build
   speed, while retaining a fast development build mode.

The compiler is written in Rust. Rust provides algebraic data types and pattern
matching for syntax trees and intermediate representations, memory safety for the
compiler itself, low-level control for the future runtime and garbage collector,
and direct access to the Rust-native Cranelift ecosystem. Unsafe Rust will be
isolated behind small runtime interfaces when raw pointers, stack roots, or
executable memory eventually require it.

## Current status

Kal currently implements its first complete test-driven walking skeleton. This
program is valid today:

```lisp
(defn main ((args (Array String))) -> Int
  (do
    (print "Hello, world!")
    0))
```

Build and run the checked-in example with:

```console
cargo run -- build examples/hello.kal -o hello
./hello
```

The resulting file is a native, dynamically linked, position-independent amd64
ELF executable. It writes exactly `Hello, world!` without a trailing newline and
exits with status `0`. It does not contain the compiler and does not need Rust to
run.

The current implementation includes:

- S-expression tokenization and parsing
- Integer and string literals
- Explicitly typed `main`
- The `Int`, `String`, `Unit`, and `(Array String)` type forms needed by `main`
- The `do` sequencing form
- `print` for string literals
- Minimal semantic validation
- Cranelift object generation
- Atomic linking through the system `cc` driver
- Diagnostics that do not leave a new executable after compilation fails

Everything else described below is the agreed language direction and will be
introduced through tests rather than being treated as already implemented.

## Source files and command line

The language is named **Kal**, the compiler executable is `kal`, and the canonical
source extension is `.kal`. The extension is a convention rather than a hard
restriction: an explicitly supplied file with another extension can still be
compiled.

The primary compiler interface is:

```console
kal build program.kal
kal build program.kal -o another-name
```

Without `-o`, the output name is the source file's stem, so `program.kal` produces
`./program` in the current directory. Later milestones add:

```console
kal run program.kal
kal repl
```

Development builds will initially link dynamically. Once static runtime support
is ready, release builds will default to standalone static binaries while keeping
an explicit dynamic-linking option.

## Program entry point

Normal ahead-of-time compiled programs define this entry point:

```lisp
(defn main ((args (Array String))) -> Int
  ...)
```

`args` contains the command-line arguments and the returned `Int` becomes the
process exit status. Free-standing expressions are reserved for the future REPL;
source files use a named, typed entry point.

Function parameter types and return types are written inline with the function:

```lisp
(defn add ((left Int) (right Int)) -> Int
  (+ left right))
```

This keeps each function's complete contract in one form and lets the checker
register and verify it without matching a separate type declaration.

## Static type system

Kal requires explicit function parameter and return types. Type inference is not
part of the language direction. Local implementation details may use obvious
expression types, but function boundaries remain explicit.

The goals of this choice are:

- Fast and comparatively simple type checking
- Local, predictable diagnostics
- No runtime type checks for statically known operations
- Straightforward lowering to unboxed native representations
- Identical runtime performance to an equivalent inferred static type system

Kal is not gradually typed. Values do not silently become a universal dynamic
type, and normal function calls do not perform runtime type dispatch. A dynamic
value facility could only be introduced later as an explicit type with explicit
costs.

The initial core types will grow to include:

- `Int`: signed 64-bit integer
- `Float64`: IEEE-754 double-precision floating point
- `Bool`
- `Unit`
- `String`
- `(Array T)`
- Functions
- User-defined product types and tagged sum types

Primitive values remain unboxed whenever their static type permits it. Tagged
representations are used only where the language semantics genuinely require a
runtime choice, such as a sum type.

## Numeric semantics

`Int` is always a signed 64-bit value. Ordinary integer overflow traps
predictably in every build mode; release optimization must not silently change
overflow behavior. Explicit wrapping or checked arithmetic operations can be
added separately.

`Float64` is distinct from `Int`. Mixed arithmetic is rejected:

```lisp
(+ 10 2.5) ; type error
```

The programmer must request the conversion:

```lisp
(+ (float64 10) 2.5)
```

This avoids hidden precision loss and makes generated instructions predictable.
Integer and floating-point addition use different machine instructions, but both
are exposed through the same source-level operation once their operand types are
known at compile time.

## Traits and generic code

Traits are Kal's interface-like abstraction. A trait names a capability that a
type can implement; the compiler resolves implementations statically and
specializes generic code. Traits do not imply virtual runtime dispatch.

The exact trait syntax will be driven by tests, but the intended model is:

```lisp
; Illustrative, not implemented syntax.
(trait Add
  (fn add ((left Self) (right Self)) -> Self))

(impl Add for Int
  ...)

(impl Add for Float64
  ...)
```

A generic function declares its type parameters and trait requirements
explicitly:

```lisp
; Illustrative, not implemented syntax.
(defn sum [T where (implements T Add)]
  ((left T) (right T)) -> T
  (add left right))
```

Using `sum` with `Int` and `Float64` causes monomorphization: the compiler emits a
specialized native version for each concrete type. Calls therefore need no
boxing, tag checks, virtual table, or runtime method lookup.

Implementation will begin with built-in statically selected arithmetic. The
general user-defined trait abstraction will only be extracted after multiple
concrete operations demonstrate what the shared mechanism must support.

## Evaluation, sequencing, and immutability

Kal uses ordinary eager function evaluation. `do` evaluates its expressions in
order and has the type and value of its final expression:

```lisp
(do
  (print "working")
  0)
```

Bindings and core data structures start immutable. This keeps early semantics
simple, enables stronger optimizations, and avoids adding mutation before there
is a concrete use case. Explicit mutable cells or collections may be introduced
later; Scheme-style unrestricted `set!` is not part of the initial core.

## Memory management

Heap-allocated values will use automatic, precise tracing garbage collection so
Kal retains normal Lisp ergonomics without imposing an ownership system on its
users.

The collector roadmap is:

1. Start with a simple precise, non-moving tracing collector.
2. Track roots explicitly at first, using a shadow-stack-style mechanism where
   necessary.
3. Preserve root information in the compiler IR.
4. Move to compiler-generated stack maps and more advanced collectors only when
   profiling demonstrates the need.

The initial `Int`, `Float64`, and `Bool` representations do not require garbage
collection. Runtime and collector code will be separated from the compiler when
the first heap feature makes that boundary necessary.

## Ahead-of-time compilation and the REPL

Ahead-of-time compilation comes first. The normal pipeline is:

```text
source.kal
  -> parser
  -> name resolution
  -> static type checking
  -> typed Kal IR
  -> optimization passes
  -> Cranelift IR
  -> amd64 object file
  -> system linker
  -> native ELF executable
```

A REPL remains an important language goal, but it will not be backed by a second
interpreter with potentially different semantics. The future `kal repl` will
incrementally compile forms through the same frontend and typed IR, using
Cranelift's JIT facilities to execute native code and retain definitions between
forms.

This produces a staged execution model:

1. AOT compiler and standalone binaries
2. Shared-IR native JIT
3. Interactive REPL

Runtime `eval` inside ordinary compiled programs is not an initial requirement.

## Macros

Macros are deferred until functions, types, native code generation, and the
runtime are stable. The compiler architecture reserves a macro-expansion stage
between parsing and type checking.

Macros are eventually useful because an eager function receives evaluated
values, while a macro can transform unevaluated syntax. For example, an `unless`
form can expand into an `if` without evaluating its body unconditionally.

Kal's eventual macros will be hygienic so generated bindings cannot accidentally
capture surrounding variables. Expanded code is statically checked like
hand-written code, and macro expansion has no runtime cost.

## Native code, linking, and deployment

Cranelift is the first and current backend. It is sufficiently small and
Rust-native for the early compiler while supporting both object generation and
the future JIT.

The current linker path is dynamic:

```text
program
  = compiled Kal machine code
  + referenced Kal runtime code
  + references to the Linux loader and system libraries
```

Dynamic linking keeps the first toolchain and executable small. A later
musl-compatible static path will produce a larger single-file executable that
contains its required startup and user-space library code. A static executable
still depends on the Linux kernel ABI; it does not contain an operating system.

Optimized release builds will eventually default to static linking for easy
deployment, while development builds can remain dynamic. This choice primarily
affects size and deployment rather than arithmetic or function-call performance.

Direct amd64 instruction emission is not the production bootstrap path. It may
become an educational backend later, but implementing instruction selection,
register allocation, relocations, and ELF writing would otherwise compete with
the language itself.

LLVM is also deferred. It will only be considered as an additional optimized AOT
backend if representative benchmarks and profiling show that backend code
generation remains the limiting factor after Kal-specific optimization.

## Optimization direction

Kal prioritizes fast generated programs. Development and release builds will use
different optimization budgets while preserving exactly the same language
semantics.

The optimizer will grow through measured, individually tested passes:

- Constant folding
- Dead-code elimination
- Inlining
- Proper tail-call handling
- Closure conversion
- Escape analysis
- Generic specialization and monomorphization
- Unboxing

The initial typed expression representation stays small. SSA IR will be
introduced when the first optimization spanning multiple expressions requires
it, rather than as an abstraction created in advance.

Benchmarks are separate from correctness tests. They guide optimization work and
prevent meaningful release-performance regressions without placing flaky timing
assertions in the ordinary test suite.

## Development

Kal is developed with strict red-green-refactor TDD. Black-box compile-and-run
tests are the primary executable specification for language behavior.

Repository workflow, architecture guardrails, test placement, and required
quality checks live in [`AGENTS.md`](AGENTS.md). The README remains the source of
truth for Kal's user-visible language decisions.

## Roadmap

### 1. Native walking skeleton — implemented

- Full typed Hello World source
- Exact stdout behavior
- Native amd64 ELF output
- Cranelift object emission
- Dynamic system linking
- Invalid-program diagnostic acceptance test

### 2. Typed core

- Source locations and structured diagnostics
- Name resolution and lexical bindings
- `Bool`, conditionals, arithmetic, and comparisons
- Function calls and recursion
- Actual construction of `args` from native `argc` and `argv`
- `Float64` and explicit numeric conversions

### 3. General-purpose runtime

- Immutable arrays and strings
- Console and file I/O
- Closures
- Product types and tagged sum types
- Separately linked Rust runtime
- Precise non-moving tracing garbage collector

### 4. Generic programming and optimization

- Built-in arithmetic overloads
- User-defined traits
- Explicit generic parameters and constraints
- Monomorphization
- SSA IR and measured optimization passes
- Fast development and aggressively optimized release modes

### 5. Distribution and interactive use

- Static amd64 Linux release binaries
- Dynamic-link override
- Shared-IR Cranelift JIT
- Native REPL
- Hygienic macros
- Benchmark-gated evaluation of an LLVM backend

## Deliberately deferred

The initial language does not include:

- Type inference or gradual typing
- Implicit numeric promotion
- Runtime multiple dispatch
- Mutable bindings or collections
- Macros
- A REPL or runtime `eval`
- A hand-written amd64 production backend
- Self-hosting
- Targets other than amd64 Linux

These are explicit boundaries, not accidental omissions. New capabilities will
enter the language only with a concrete test and a clear effect on its semantics,
runtime, and optimization model.

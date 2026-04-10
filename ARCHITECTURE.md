# VM-Engine Architecture

> **Version:** 0.2.0 | **Updated:** 2026-04-10
> **Crate:** `vm_engine`

Universal intermediate representation and analysis framework for bytecode virtual machines. Converts any VM bytecode into one structured IR format, then analyzes, recovers control flow, and optionally executes it.

---

## Overview

Every anti-bot VM — FunCaptcha, Cloudflare, Kasada, Shape F5, DataDome, Incapsula — hides code inside a custom interpreter. The bytecode format, dispatch method, and encoding differ. But after decoding, every VM does the same operations: arithmetic, bitwise math, property access, function calls, branches, and loops.

VM-Engine defines ONE intermediate representation that all VMs convert into. Once in this format, the same analysis, structure recovery, and execution tools work regardless of which VM the bytecode came from.

```text
Raw bytecode (per-VM, any format)
    |
    v
[User-written decoder]         ← per-VM: ~300-500 lines
    |
    v
IR (Module → Function → Block → Instruction)
    |
    +--> [graph]      → CFG, dominators, loops, call graph
    |       |
    |       v
    |    [structure]   → if/else, while, expressions → readable output
    |
    +--> [exec]       → interpreter, hooks, tracing, data extraction
    |
    +--> [disasm]     → IR listing, DOT graphs, summary stats
```

---

## Crate Structure

Single crate. Seven modules. Strict dependency direction. No cycles.

```text
src/
├── lib.rs              Public API and re-exports
├── error.rs            Error types
├── value/              Layer 0 — JS-compatible values (depends: nothing)
│   ├── mod.rs          Value enum, ObjectId, ClosureId
│   ├── ops.rs          BinaryOp (22), UnaryOp (6), binary(), unary()
│   └── coerce.rs       to_boolean, to_number, to_int32, to_string
├── ir/                 Layer 1 — Intermediate representation (depends: value)
│   ├── mod.rs          Module, Function, Block, Instruction, Var, BlockId, FuncId
│   ├── opcode.rs       OpCode (47 operations), Terminator (7 variants)
│   ├── operand.rs      Operand, SourceLoc
│   ├── builder.rs      IrBuilder — ergonomic IR construction
│   ├── display.rs      Pretty-print IR
│   └── validate.rs     Well-formedness checks
├── exec/               Layer 2 — IR interpreter (depends: ir, value)
│   ├── mod.rs          Interpreter: run(), step(), step_block()
│   ├── heap.rs         Object heap, CallableKind (Native, Closure, IrFunction)
│   ├── state.rs        State, Cursor, CallFrame
│   ├── scope.rs        ScopeChain — lexical scope with parent links
│   ├── hooks.rs        Hook trait — bridge to browser APIs
│   ├── trace.rs        Structured event recording with filters
│   └── breakpoint.rs   Breakpoint conditions
├── web/                Layer 3 — Browser environment (depends: exec, value)
│   ├── mod.rs          WebConfig, install_all()
│   ├── globals.rs      window, Uint8Array, parseInt, isNaN, isFinite
│   ├── document.rs     document, location, createElement, body
│   ├── navigator.rs    navigator, webdriver
│   ├── encoding.rs     btoa, atob
│   ├── json.rs         JSON.stringify, JSON.parse
│   ├── math.rs         Math.* (35 methods + 8 constants)
│   ├── string_utils.rs String.fromCharCode
│   ├── timing.rs       Date.now, performance.now, performance.timing
│   ├── random.rs       Math.random, crypto.getRandomValues
│   ├── screen.rs       screen.*
│   └── console.rs      console.log (no-op)
├── graph/              Layer 4 — CFG analysis (depends: ir)
│   ├── mod.rs          Cfg, build_cfg()
│   ├── block.rs        CfgBlock, EdgeKind
│   ├── dominator.rs    DominatorTree, compute_dominators(), post_dominators()
│   ├── loops.rs        LoopInfo, detect_loops()
│   └── callgraph.rs    CallGraph, build_call_graph()
├── structure/          Layer 5 — Structure recovery (depends: ir, graph)
│   ├── mod.rs          recover_to_string()
│   ├── ast.rs          Stmt, Expr — structured AST
│   ├── region.rs       Loop, if/else, switch pattern detection
│   ├── expr.rs         Expression reconstruction (inline single-use vars)
│   └── simplify.rs     Constant folding, identity removal
└── disasm/             Layer 6 — Output formatting (depends: ir, graph, structure)
    ├── mod.rs          disasm_ir(), disasm_structured(), disasm_summary()
    ├── listing.rs      IR listing with source PC annotations
    ├── dot.rs          Graphviz DOT output for CFG and call graph
    ├── structured.rs   Recovered pseudo-JS output
    └── summary.rs      Per-function statistics
```

### Dependency Graph

```text
disasm ────────→ ir + graph + structure
structure ─────→ ir + graph
graph ─────────→ ir
web ───────────→ exec + value
exec ──────────→ ir + value
ir ────────────→ value
value ─────────→ nothing
```

No cycles. Each layer depends only on layers below it.

---

## Module Details

### value — JS Type System

Every VM targets JavaScript. Every runtime value is one of these types.

```rust
enum Value {
    Number(f64), String(String), Bool(bool), Null, Undefined,
    Object(ObjectId), Array(Vec<Value>), Bytes(Vec<u8>), Closure(ClosureId),
}
```

ECMAScript coercion (`to_number`, `to_boolean`, `to_int32`, `to_string`) and operators (22 binary, 6 unary) follow the spec.

### ir — Universal Instruction Set

Structured from creation: Module → Function → Block → Instruction.

- **47 opcodes** in four categories: pure (26), memory (10), control (2), data (4)
- **7 terminators**: Jump, BranchIf, Switch, Return, Halt, Throw, Unreachable
- **IrBuilder**: ergonomic API for decoder authors — handles variable numbering, block management
- **SourceLoc**: every instruction carries a back-reference to the original bytecode PC
- **ID types** (`Var`, `BlockId`, `FuncId`, `ObjectId`) are opaque — `pub(crate)` fields prevent invalid construction

### exec — IR Interpreter

Runs the IR with hooks, tracing, and breakpoints.

- **Interpreter**: `run()`, `step()`, `step_block()`, `step_over()`, `step_out()`
- **Heap**: arena-allocated objects with prototype chains and callable support
- **Hook trait**: bridge to browser APIs — exec knows nothing about browsers
- **TraceRecorder**: structured events (not println), filterable, bounded memory
- **Breakpoints**: by instruction, source PC, variable write, property access, or custom predicate

### web — Browser Environment

Installs browser globals onto the heap. Configurable (deterministic timing, fixed random seed).

### graph — Control Flow Analysis

`build_cfg(function)` → CFG with typed edges. Then:
- **Dominator tree** (Cooper-Harvey-Kennedy algorithm)
- **Post-dominator tree** (for if/else merge detection)
- **Natural loop detection** (back edges + body collection)
- **Call graph** (inter-function analysis)

### structure — Decompilation

Recovers if/else, while, do-while, loops from CFG + dominators + loops. Expression reconstruction inlines single-use variables. Simplification folds constants and removes identities.

### disasm — Output Formatting

Four modes: IR listing (with source PCs), structured pseudo-JS, Graphviz DOT, summary statistics.

---

## Design Principles

| Principle | Meaning |
|-----------|---------|
| IR-first | Everything operates on the IR, not raw bytecode |
| Understand before execute | Decode and analyze WITHOUT running. Execution is optional |
| No per-VM code in src/ | Zero knowledge of any specific VM. Users write decoders |
| Separate pure from effectful | Pure ops, memory ops, and control ops are distinct categories |
| Opaque IDs | Var, BlockId, FuncId, ObjectId can't be constructed outside the crate |

---

## User Workflow

### 1. Write a Decoder (~300-500 lines per VM)

```rust
fn decode_my_vm(bytecode: &[u8]) -> Module {
    let mut builder = IrBuilder::new();
    builder.begin_function("main");
    builder.create_and_switch("entry");
    // match raw opcodes → emit IR instructions
    builder.end_function();
    builder.build()
}
```

### 2. Analyze

```rust
let module = decode_my_vm(&bytecode);
for func in &module.functions {
    let cfg = graph::build_cfg(func);
    let doms = graph::dominator::compute_dominators(&cfg);
    let loops = graph::loops::detect_loops(&cfg, &doms);
    // Now: see functions, loops, branches without running anything
}
```

### 3. Read or Run

```rust
// Read the structure
println!("{}", disasm::disasm_structured(&module));

// Or execute with browser environment
let mut interp = Interpreter::new(&module)?;
web::install_all(&mut interp.state.heap, global, &WebConfig::default());
interp.run()?;
```

---

## Test Coverage

200 library tests + 4 doc tests. All modules have test suites:

| Module | Tests |
|--------|-------|
| value (ops, coerce) | 52 |
| ir (builder, validate, display) | 19 |
| exec (interpreter, heap, scope) | 35 |
| web (all sub-modules) | 53 |
| graph (cfg, dominator, loops, callgraph) | 22 |
| structure (region, expr, simplify, ast) | 16 |
| disasm (listing, dot, summary, structured) | 7 |

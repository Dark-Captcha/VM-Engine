# VM-Engine Core Architecture

> **Version:** 0.1.0 | **Status:** Draft | **Updated:** 2026-04-09
> **Module:** `vm_engine_core`

Universal intermediate representation and analysis framework for bytecode virtual machines. Converts any VM bytecode into one structured format, then analyzes, recovers control flow, and optionally executes it.

---

| #   | Section                                             |
| --- | --------------------------------------------------- |
| 1   | [Overview](#overview)                               |
| 2   | [Why This Exists](#why-this-exists)                 |
| 3   | [Design Principles](#design-principles)             |
| 4   | [Module Map](#module-map)                           |
| 5   | [value](#value)                                     |
| 6   | [ir](#ir)                                           |
| 7   | [graph](#graph)                                     |
| 8   | [structure](#structure)                             |
| 9   | [exec](#exec)                                       |
| 10  | [What Core Does Not Contain](#what-core-does-not-contain) |
| 11  | [User Workflow](#user-workflow)                      |
| 12  | [Research Required](#research-required)              |
| 13  | [File Map](#file-map)                               |

---

## Overview

Every anti-bot VM — FunCaptcha, Cloudflare, Kasada, Shape F5, DataDome, Incapsula — hides code inside a custom interpreter. The bytecode format, dispatch method, and encoding differ across vendors. But after decoding, every VM does the same operations: arithmetic, bitwise math, property access, function calls, branches, and loops.

Core defines ONE intermediate representation that all VMs convert into. Once in this format, the same analysis, structure recovery, and execution tools work regardless of which VM the bytecode came from.

```text
Raw bytecode (per-VM, any format)
    |
    v
[User-written decoder]         ← per-VM: ~300-500 lines
    |
    v
IR (Module → Function → Block → Instruction)
    |
    +--> [graph]     → CFG, dominators, loops, call graph
    |       |
    |       v
    |    [structure]  → if/else, while, switch, functions
    |       |
    |       v
    |    readable structured output (via vm-engine-disasm)
    |
    +--> [exec]      → interpreter, debugger, data extraction
```

---

## Why This Exists

The previous `vm-engine` library failed on a concrete task: extracting 8 obfuscated keys from the DataDome PLV3 VM. The failure was architectural.

| Problem | What Happened | Consequence |
| --- | --- | --- |
| No IR | Opcode handlers operated directly on raw bytecode | 2,136 lines of hand-written closures for ONE VM |
| Executor-first design | The library was built around a step loop, not analysis | Had to run 82,600 steps blind before understanding anything |
| No structure | CFG existed but no loop detection, no if/else recovery | Could not see the 3 functions or the cipher algorithm |
| Missing value types | No Array, no Function, no Bytes in Value enum | S-boxes (256-byte arrays) faked through string-keyed heap objects |
| Analysis was optional | Placed at level 5 of 7 — an afterthought | Analysis was never used; debugging relied on ad-hoc env-var tracing |
| Mixed abstraction | Raw bytecode reading (dispatch, readers) mixed with VM semantics | Core was neither a good reader library nor a good analysis library |

The correct approach: understand the bytecode structure FIRST (decode → analyze → see functions/loops/algorithm), THEN optionally execute to extract data. Not the reverse.

---

## Design Principles

| Principle | Meaning |
| --- | --- |
| IR-first | The IR is the core concept. Everything operates on the IR, not raw bytecode. |
| Never linear | The IR is structured into Module → Function → Block → Instruction from the start. No flat instruction arrays. |
| Understand before execute | Decode and analyze structure WITHOUT running. Execution is optional, not required. |
| Separate pure from effectful | Pure operations (add, xor, compare) have no side effects. Memory operations (load/store property) mutate state. Control operations (jump, branch, call) change flow. Three distinct categories. |
| No per-VM code in core | Core contains zero knowledge of any specific VM. The user writes a decoder per VM. Core provides the target format and universal tools. |
| Break and rebuild | Each module has clean interfaces. Rewriting one module does not require rewriting others. |

---

## Module Map

Five modules. Strict dependency direction. No cycles.

```text
value       (no dependencies)
  ^
  |
  ir        (depends on value)
  ^    ^
  |    |
graph  exec (both depend on ir + value; independent of each other)
  ^
  |
structure   (depends on ir + graph)
```

| Module | One Job | Input | Output |
| --- | --- | --- | --- |
| `value` | JS-compatible type system and pure operators | Two values + an operation | One value |
| `ir` | Universal instruction set and program structure | Decoder calls to IrBuilder | Module (functions, blocks, instructions) |
| `graph` | Control flow analysis | IR Function | CFG, dominator tree, loop info, call graph |
| `structure` | Recover readable control flow | IR Function + CFG + loops | AST (if/else, while, switch, functions) |
| `exec` | Interpret the IR | IR Module + hooks | Runtime state, trace events, extracted data |

---

## value

The JavaScript type system. Every VM targets JavaScript, so every value in every VM is one of these types. Every operator follows ECMAScript semantics.

### Types

```rust
enum Value {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    Undefined,
    Object(ObjectId),
    Array(Vec<Value>),
    Bytes(Vec<u8>),
    Closure(ClosureId),
}
```

| Variant | Why It Exists | What Failed Without It |
| --- | --- | --- |
| `Number(f64)` | All JS numbers are f64 | — |
| `String` | Property keys, string operations, btoa input | — |
| `Bool` | Conditions, flags | — |
| `Null` / `Undefined` | JS null semantics, missing property returns | — |
| `Object(ObjectId)` | Heap-allocated objects with properties | — |
| `Array(Vec<Value>)` | S-boxes (256 entries), Kasada E[] (185k), bytecode buffers | Old library faked arrays as heap objects with string keys `"0"`, `"1"`, ... `"255"`. Slow and broke property iteration. |
| `Bytes(Vec<u8>)` | Cipher buffers, binary data, raw bytecode chunks | Old library had no binary type. Cipher operations on individual bytes required f64 round-trips. |
| `Closure(ClosureId)` | Incapsula combinators are functions that return functions. Kasada handlers are callable. | Old library stored callables only in heap objects. No way to pass functions as values through the IR. |

### Coercion

ECMAScript type coercion, implemented once, used everywhere.

| Function | ECMAScript Section | Used By |
| --- | --- | --- |
| `to_boolean(val)` | 7.1.2 | BranchIf condition evaluation, LogicalNot |
| `to_number(val)` | 7.1.3 | All arithmetic and comparison operators |
| `to_int32(val)` | 7.1.5 | BitAnd, BitOr, BitXor, Shl, Shr, BitNot |
| `to_uint32(val)` | 7.1.6 | UShr (unsigned right shift) |
| `to_string(val)` | 7.1.12 | String concatenation via Add, property key coercion |

### Operators

One function per category. Not one file per operator.

`binary(op, left, right) -> Value` handles all 22 binary operations in one match. `unary(op, val) -> Value` handles all 6 unary operations in one match.

| Category | Operations | Count |
| --- | --- | --- |
| Arithmetic | Add, Sub, Mul, Div, Mod, Pow | 6 |
| Bitwise | BitAnd, BitOr, BitXor, Shl, Shr, UShr | 6 |
| Comparison | Eq, Neq, StrictEq, StrictNeq, Lt, Gt, Lte, Gte | 8 |
| Unary arithmetic | Neg, Pos | 2 |
| Unary logic | LogicalNot, BitNot, TypeOf, Void | 4 |

Add is special: string concatenation when either operand is a string, numeric addition otherwise. This matches ECMAScript and is required by every VM.

### Files

| File | Contents |
| --- | --- |
| `mod.rs` | Value enum, constructors, `type_name()`, Display, PartialEq |
| `coerce.rs` | `to_boolean`, `to_number`, `to_int32`, `to_uint32`, `to_string` |
| `ops.rs` | BinaryOp (22 variants), UnaryOp (6 variants), `binary()`, `unary()` |

---

## ir

The intermediate representation. The universal format every VM converts into. Structured from creation — never a flat instruction list.

### Hierarchy

```text
Module
└── Function[]
    ├── id: FuncId
    ├── name: String
    ├── params: Vec<Var>
    ├── entry: BlockId
    └── Block[]
        ├── id: BlockId
        ├── body: Vec<Instruction>
        └── terminator: Terminator
```

A Module contains functions. A Function contains blocks. A Block contains instructions and ends with exactly one terminator. No block exists without a function. No instruction exists without a block.

### Instruction

```rust
struct Instruction {
    result: Option<Var>,       // output variable (%0, %1, %2...), None for void ops
    op: OpCode,                // what operation
    operands: Vec<Operand>,    // inputs
    source: Option<SourceLoc>, // original bytecode location (for debugging)
}
```

Each instruction that produces a value assigns it to a fresh variable. Variables are defined once. This makes data flow trivial to trace: every use points to exactly one definition.

### Operand

```rust
enum Operand {
    Var(Var),           // reference to another instruction's result
    Const(Value),       // literal value
    Block(BlockId),     // jump target
    Func(FuncId),       // function reference
}
```

### Terminator

Every block ends with exactly one terminator. The terminator determines control flow — which block runs next.

```rust
enum Terminator {
    Jump { target: BlockId },
    BranchIf { cond: Var, if_true: BlockId, if_false: BlockId },
    Switch { value: Var, cases: Vec<(Value, BlockId)>, default: BlockId },
    Return { value: Option<Var> },
    Halt,
    Throw { value: Var },
    Unreachable,
}
```

| Terminator | When | Example VM Usage |
| --- | --- | --- |
| `Jump` | Unconditional goto | Every VM |
| `BranchIf` | Two-way conditional | FunCaptcha JMPF, Cloudflare CJMP, DataDome COND_JUMP |
| `Switch` | Multi-way branch | Comparison cascades, jump tables |
| `Return` | Exit function with value | Every VM |
| `Halt` | Stop execution entirely | DataDome HALT, Cloudflare HALT |
| `Throw` | Raise exception | Cloudflare THROW, FunCaptcha TRY |
| `Unreachable` | Dead code marker | After unconditional throw or infinite loop |

### OpCode

47 operations organized into four categories.

```rust
enum OpCode {
    // ── Pure: value in, value out, no side effects ──────────
    Add, Sub, Mul, Div, Mod, Pow,
    BitAnd, BitOr, BitXor, Shl, Shr, UShr, BitNot,
    Eq, Neq, StrictEq, StrictNeq, Lt, Gt, Lte, Gte,
    Neg, Pos, LogicalNot, TypeOf, Void,

    // ── Memory: reads or writes state ───────────────────────
    LoadProp,           // %r = obj[key]
    StoreProp,          // obj[key] = val
    DeleteProp,         // delete obj[key]
    HasProp,            // key in obj
    LoadIndex,          // %r = arr[i]
    StoreIndex,         // arr[i] = val
    LoadScope,          // %r = scope_lookup(name)
    StoreScope,         // scope_set(name, val)
    NewObject,          // %r = {}
    NewArray,           // %r = []

    // ── Control: changes execution flow ─────────────────────
    Call,               // %r = func(args...)
    CallMethod,         // %r = obj.method(args...)

    // ── Data: define or select values ───────────────────────
    Const,              // %r = literal value
    Param,              // %r = function parameter N
    Phi,                // %r = merge from multiple predecessor blocks
    Move,               // %r = %other (explicit copy)
}
```

Control flow operations (Jump, BranchIf, Return, etc.) live in the `Terminator` enum, not in OpCode. This enforces the rule: terminators end blocks, instructions do not.

| Category | Count | Side Effects | Example |
| --- | --- | --- | --- |
| Pure | 26 | None | `%2 = BitXor %0, %1` |
| Memory | 10 | Reads/writes heap or scope | `%3 = LoadProp %obj, "length"` |
| Control | 2 | Invokes function, may diverge | `%4 = Call %func, [%arg0, %arg1]` |
| Data | 4 | None (definition only) | `%0 = Const 255` |

### How Per-VM Opcodes Map to IR

The decoder decomposes complex per-VM opcodes into sequences of simple IR instructions.

| Per-VM Opcode | VM | IR Instructions |
| --- | --- | --- |
| `XOR_STORE_PUSH2` | DataDome PLV3 | `BitXor` → `StoreScope` → `LoadScope` → `LoadScope` |
| `NARR_SGET_PSET_VMCALL` | Kasada | `NewArray` → `LoadScope` → `StoreProp` → `Call` |
| `PUSH_REG_PROP_AND_IMM` | DataDome PLV3 | `LoadProp` → `BitAnd` (with Const) |
| `ADD_RE` / `ADD_ER` / `ADD_RR` / `ADD_EE` | Kasada | All become one `Add` |
| `E[d] = E[f](E[a])` | Incapsula | `Call` |
| `tI.bind(this, 246)` | Cloudflare bootstrap | Decoder resolves at decode time — no IR instruction needed |

### IrBuilder

What decoder authors use to construct IR. Provides ergonomic methods that handle variable numbering, block management, and validation automatically.

```rust
let mut module = IrBuilder::new();

let f = module.begin_function("cipher_round", vec!["sbox", "input", "key"]);

let entry = f.begin_block("entry");
let v0 = entry.emit(LoadIndex, &[f.param(0), f.param(1)]);  // sbox[input]
let v1 = entry.emit(BitXor, &[v0, f.param(2)]);              // ^ key
let limit = entry.emit(Const, &[Operand::Const(Value::number(255.0))]);
let cond = entry.emit(Gt, &[v1, limit]);                     // > 255?
let overflow = f.begin_block("overflow");
let done = f.begin_block("done");
entry.branch_if(cond, overflow.id(), done.id());

let v2 = overflow.emit(BitAnd, &[v1, limit]);                // & 0xFF
overflow.jump(done.id());

let v3 = done.phi(&[(entry.id(), v1), (overflow.id(), v2)]);
done.ret(Some(v3));
```

### SourceLoc

Every IR instruction can carry a back-reference to the original bytecode.

```rust
struct SourceLoc {
    pc: usize,                  // byte offset in original bytecode
    original_opcode: Option<u16>, // raw opcode number before decoding
}
```

This is critical for debugging. When stepping through the IR, the user sees both the IR instruction AND which raw bytecode it came from.

### Files

| File | Contents |
| --- | --- |
| `mod.rs` | Module, Function, Block, Instruction, Var, FuncId, BlockId |
| `opcode.rs` | OpCode enum (47 operations), Terminator enum |
| `operand.rs` | Operand enum, SourceLoc |
| `builder.rs` | IrBuilder, FunctionBuilder, BlockBuilder |
| `validate.rs` | Check well-formedness: all Var refs resolve, every block has a terminator, no orphan blocks |
| `display.rs` | Pretty-print IR as readable text |

---

## graph

Control flow analysis built from the IR. Automatic — the user calls `build_cfg(function)` and gets a complete analysis.

### CFG

```rust
struct Cfg {
    blocks: Vec<CfgBlock>,
    edges: Vec<(BlockId, BlockId, EdgeKind)>,
    entry: BlockId,
}

enum EdgeKind {
    Fallthrough,
    Jump,
    TrueBranch,
    FalseBranch,
    SwitchCase(Value),
    Exception,
}
```

Built directly from the IR block structure and terminators. Each IR Block becomes a CfgBlock. Each terminator produces edges.

### Dominator Tree

Answers: "which block MUST execute before block B?" Computed with the Cooper-Harvey-Kennedy iterative algorithm.

```rust
struct DominatorTree {
    idom: BTreeMap<BlockId, BlockId>,  // immediate dominator of each block
}
```

`dominates(a, b)` — does block A always execute before block B? Used by loop detection and structure recovery.

Post-dominator tree answers the reverse: "which block MUST execute after block B?" Built by running the dominator algorithm on the reversed CFG with a synthetic exit node. Used by if/else recovery (the merge point post-dominates the branch point).

### Loop Detection

Finds natural loops in the CFG using dominators.

```rust
struct LoopInfo {
    header: BlockId,                 // the block where the loop starts
    body: BTreeSet<BlockId>,         // all blocks inside the loop
    back_edges: Vec<(BlockId, BlockId)>, // edges that jump back to header
    exits: Vec<BlockId>,             // blocks that leave the loop (break targets)
    parent: Option<LoopId>,          // enclosing loop (for nested loops)
    depth: usize,                    // nesting level (0 = outermost)
}
```

Algorithm: a back edge is any edge where the target dominates the source. For each back edge, collect all blocks that can reach the source without passing through the header. Those blocks form the loop body.

The old library had zero loop detection. This was a critical missing piece — without it, you cannot distinguish a loop from a sequence of jumps.

### Call Graph

Which functions call which. Built by scanning all `Call` and `CallMethod` instructions across every function in the Module.

```rust
struct CallGraph {
    callers: BTreeMap<FuncId, Vec<FuncId>>,  // who calls this function
    callees: BTreeMap<FuncId, Vec<FuncId>>,  // who does this function call
    roots: Vec<FuncId>,                       // functions with no callers (entry points)
    leaves: Vec<FuncId>,                      // functions that call nothing
}
```

DataDome PLV3 has 3 functions. Kasada has 1,022. Shape F5 has 579. The call graph tells you which functions matter and how they connect before you look at any single function.

### Files

| File | Contents |
| --- | --- |
| `mod.rs` | Cfg, `build_cfg()` |
| `block.rs` | CfgBlock, EdgeKind |
| `dominator.rs` | DominatorTree, `compute_dominators()`, `compute_post_dominators()` |
| `loops.rs` | LoopInfo, LoopId, `detect_loops()` |
| `callgraph.rs` | CallGraph, `build_call_graph()` |

---

## structure

Recovers high-level control flow from the CFG. Turns a graph of blocks and edges into nested if/else, while, switch, and function declarations.

This is what makes output readable. Without it, the result is a flat list of labeled blocks with gotos. With it, the result looks like the decompiled output in the vm-101 walkthroughs.

### AST

The output format. Two types: statements and expressions.

```rust
enum Stmt {
    Assign { dst: String, expr: Expr },
    If { cond: Expr, then_body: Vec<Stmt>, else_body: Option<Vec<Stmt>> },
    While { cond: Expr, body: Vec<Stmt> },
    DoWhile { body: Vec<Stmt>, cond: Expr },
    Loop { body: Vec<Stmt> },                          // infinite loop (break to exit)
    ForRange { var: String, from: Expr, to: Expr, body: Vec<Stmt> },
    Switch { value: Expr, cases: Vec<SwitchCase>, default: Option<Vec<Stmt>> },
    Break,
    Continue,
    Return(Option<Expr>),
    Throw(Expr),
    TryCatch { body: Vec<Stmt>, catch_var: String, catch_body: Vec<Stmt> },
    Block(Vec<Stmt>),
    ExprStmt(Expr),
}

enum Expr {
    Var(String),
    Const(Value),
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnaryOp, operand: Box<Expr> },
    Call { func: Box<Expr>, args: Vec<Expr> },
    MethodCall { obj: Box<Expr>, method: String, args: Vec<Expr> },
    PropAccess { obj: Box<Expr>, key: Box<Expr> },
    Index { array: Box<Expr>, index: Box<Expr> },
    ArrayLit(Vec<Expr>),
    ObjectLit(Vec<(String, Expr)>),
    Ternary { cond: Box<Expr>, then_e: Box<Expr>, else_e: Box<Expr> },
}
```

### Recovery Process

```text
IR Function + CFG + Dominator Tree + Loop Info
    |
    v
[region detection]
    |  identify loop regions (header + body → While/DoWhile)
    |  identify if-then-else diamonds (branch + merge → If/Else)
    |  identify switch patterns (multi-target branch → Switch)
    |
    v
[expression reconstruction]
    |  within each block, inline single-use variables into compound expressions
    |  %0 = LoadProp %obj, "x"  →  (part of a larger expression)
    |  %1 = Const 0xFF
    |  %2 = BitAnd %0, %1       →  obj.x & 0xFF
    |
    v
[simplification]
    |  fold constants: BitAnd(Const(185), Const(171)) → Const(18)
    |  remove identity: Add(x, Const(0)) → x
    |  remove dead assignments: %5 = ... (never used) → delete
    |
    v
Stmt/Expr AST (readable structured output)
```

### Region Detection Patterns

| Pattern | CFG Shape | Recovers To |
| --- | --- | --- |
| If-then-else | Block B branches to T and F; both reach merge M where M is the immediate post-dominator of B | `If { cond, then, else }` |
| If-then (no else) | Block B branches to T and M; T reaches M; M is the immediate post-dominator of B | `If { cond, then, else: None }` |
| While loop | Back edge to header H; H branches into body or exit | `While { cond, body }` |
| Do-while loop | Body block always reaches header H; H branches to body or exit | `DoWhile { body, cond }` |
| Infinite loop | Back edge to header H; no exit condition (break to exit) | `Loop { body }` with `Break` |
| Switch | Block with N outgoing edges, or comparison cascade | `Switch { value, cases, default }` |

### Expression Reconstruction

The rule: if IR variable `%N` is defined in the same block and used exactly once, inline it into the parent expression. Otherwise, emit a separate assignment.

Before (IR):
```text
%0 = LoadProp %sbox, "0"
%1 = Const 0xFF
%2 = BitAnd %0, %1
%3 = LoadScope "key"
%4 = BitXor %2, %3
```

After (expression reconstruction):
```text
result = (sbox[0] & 0xFF) ^ key
```

Five IR instructions become one readable line. This is what makes the vm-101 walkthrough decompiled outputs useful.

### Files

| File | Contents |
| --- | --- |
| `mod.rs` | `recover()` main entry point |
| `ast.rs` | Stmt, Expr, SwitchCase enums |
| `region.rs` | If/else diamond detection, loop region detection, switch pattern detection |
| `expr.rs` | Expression reconstruction (inlining single-use variables) |
| `simplify.rs` | Constant folding, identity removal, dead assignment elimination |

---

## exec

IR interpreter. Runs the IR for debugging, tracing, and data extraction. Operates on IR instructions, not raw bytecode — every VM uses the same interpreter.

### Interpreter

```rust
struct Interpreter {
    module: Module,           // the IR program
    state: State,             // runtime state
    breakpoints: Vec<Breakpoint>,
    trace: TraceRecorder,
    hooks: Box<dyn Hook>,    // bridge to browser APIs (vm-engine-web)
}
```

Execution methods:

| Method | Behavior |
| --- | --- |
| `step()` | Execute one IR instruction |
| `step_block()` | Execute until current block's terminator |
| `step_over()` | Execute until returning to current call depth |
| `step_out()` | Execute until current function returns |
| `run()` | Execute until halt, breakpoint, or limit |
| `run_until(predicate)` | Execute until predicate returns true |

### State

```rust
struct State {
    vars: HashMap<Var, Value>,        // IR variable bindings
    heap: Heap,                        // objects with properties
    scopes: ScopeChain,               // lexical scope chain
    call_stack: Vec<CallFrame>,        // function call frames
    cursor: Cursor,                    // current position in the IR
    halted: bool,
    instruction_count: u64,
}

struct Cursor {
    function: FuncId,
    block: BlockId,
    instruction: usize,               // index within block body
}

struct CallFrame {
    function: FuncId,
    return_cursor: Cursor,
    locals: HashMap<Var, Value>,
}
```

The cursor points to a specific instruction in a specific block in a specific function. Not a raw byte offset. The user always knows WHERE they are structurally.

### Heap

Objects with properties and prototype chains.

```rust
struct Heap {
    objects: Vec<Option<Object>>,
    free_list: Vec<ObjectId>,
}

struct Object {
    properties: HashMap<String, Value>,
    prototype: Option<ObjectId>,
    callable: Option<CallableKind>,
}

enum CallableKind {
    Native(NativeFn),
    IrFunction(FuncId),
    Closure { func: FuncId, captures: Vec<Value> },
}
```

### Hook

The bridge between the IR interpreter and the outside world. When execution hits a `Call` or `CallMethod` that does not resolve to an IR function, it asks the hook.

```rust
trait Hook {
    fn on_call(&mut self, name: &str, args: &[Value], heap: &mut Heap) -> Option<Value>;
    fn on_prop_get(&mut self, obj: ObjectId, key: &str, heap: &Heap) -> Option<Value>;
    fn on_prop_set(&mut self, obj: ObjectId, key: &str, val: &Value, heap: &mut Heap);
    fn on_new(&mut self, constructor: &str, args: &[Value], heap: &mut Heap) -> Option<Value>;
}
```

The `vm-engine-web` crate provides a hook that implements browser globals: `btoa`, `atob`, `Date.now`, `Math.random`, `crypto.getRandomValues`, `navigator`, `window`, etc. Core knows nothing about browsers. The hook is the boundary.

A `NullHook` (returns `None` for everything) exists for pure IR execution without browser context.

### Breakpoint

```rust
enum Breakpoint {
    AtInstruction { func: FuncId, block: BlockId, index: usize },
    AtSourcePc(usize),                          // original bytecode PC
    OnVarWrite(Var),                             // when a specific variable is assigned
    OnPropAccess { key: String },                // when a specific property is read or written
    OnCall { name: String },                     // when a specific function is called
    Custom(Box<dyn Fn(&State) -> bool>),         // user-defined predicate
}
```

`AtSourcePc` uses the `SourceLoc` on instructions to break at a position in the original bytecode. This connects raw bytecode debugging to IR-level stepping.

### Trace

Structured event recording with filters and bounded memory.

```rust
enum TraceEvent {
    Step { cursor: Cursor, op: OpCode, source_pc: Option<usize> },
    VarWrite { var: Var, value: Value },
    PropGet { obj: ObjectId, key: String, value: Value },
    PropSet { obj: ObjectId, key: String, value: Value },
    CallEnter { func: FuncId, args: Vec<Value> },
    CallReturn { func: FuncId, result: Value },
    Halted { instruction_count: u64 },
}

struct TraceRecorder {
    enabled: bool,
    capacity: usize,
    filter: TraceFilter,
    events: VecDeque<TraceEvent>,
}
```

Not env vars and `eprintln`. Structured events that can be queried, filtered, and analyzed programmatically.

### Files

| File | Contents |
| --- | --- |
| `mod.rs` | Interpreter: `run()`, `step()`, `step_block()`, `step_over()`, `step_out()` |
| `state.rs` | State, Cursor, CallFrame |
| `heap.rs` | Heap, Object, CallableKind, ObjectId |
| `scope.rs` | ScopeChain, Scope |
| `breakpoint.rs` | Breakpoint enum, matching logic |
| `trace.rs` | TraceEvent, TraceRecorder, TraceFilter |
| `hooks.rs` | Hook trait, NullHook |

---

## What Core Does Not Contain

| Excluded | Why | Where It Lives |
| --- | --- | --- |
| Raw bytecode reading (`read_byte`, `read_u16_be`, `read_varint`) | Per-VM concern. Each VM encodes operands differently. | User decoder code |
| Dispatch strategies (XOR state machine, transition table, shuffle map) | Per-VM concern. Each VM dispatches differently. | User decoder code |
| String pool decoding (XOR pairs, base64, rotated arrays) | Per-VM concern. Each VM encodes strings differently. | User decoder code |
| Browser APIs (Window, navigator, crypto, DOM) | Environment concern, not VM concern. | `vm-engine-web` |
| Text output formatting (disassembly, decompiled code rendering) | Presentation concern. | `vm-engine-disasm` |
| Bootstrap simulation (Cloudflare BIND+SWAP, Kasada Fisher-Yates) | Per-VM concern. The decoder resolves bootstrap before emitting IR. | User decoder code |

---

## User Workflow

How a user reverses a new VM using this library.

### Step 1: Write a Decoder

The user writes a function that reads raw bytecode and emits IR using the builder.

```rust
fn decode_datadome_plv3(bytecode: &[u8]) -> ir::Module {
    let mut module = IrBuilder::new();
    let main = module.begin_function("main", vec![]);
    let entry = main.begin_block("entry");

    let mut pc = 0;
    while pc < bytecode.len() {
        let raw_op = bytecode[pc];
        pc += 1;
        match raw_op {
            30 => { // ADD
                let b = /* pop from symbolic stack */;
                let a = /* pop from symbolic stack */;
                let r = entry.emit_with_source(Add, &[a, b], SourceLoc { pc: pc - 1, .. });
                /* push r to symbolic stack */
            }
            66 => { // PUSH typed value
                let (val, consumed) = read_plv_typed(bytecode, pc);
                pc += consumed;
                let r = entry.emit(Const, &[Operand::Const(val)]);
                /* push r to symbolic stack */
            }
            // ... ~100 more cases, each 3-10 lines
            _ => { /* unknown opcode handling */ }
        }
    }
    module.build()
}
```

This is ~300-500 lines for DataDome PLV3. Compare with 2,136 lines in the old approach. The decoder only translates format — it does not implement execution semantics.

### Step 2: Analyze

```rust
let module = decode_datadome_plv3(&bytecode);

for func in module.functions() {
    let cfg = graph::build_cfg(func);
    let doms = graph::compute_dominators(&cfg);
    let loops = graph::detect_loops(&cfg, &doms);
    let ast = structure::recover(func, &cfg, &doms, &loops);

    // Now: ast contains readable if/else, while, switch for this function
    // Pass to vm-engine-disasm for text output
}

let cg = graph::build_call_graph(&module);
// cg shows: main calls cipher_round, cipher_round calls nothing
```

### Step 3: Read or Run

Read the structure (via vm-engine-disasm):
```text
function main():
    sbox0 = new Uint8Array([234, 55, 132, 209, ...])
    ...
    while (i < 256):
        sbox0[i] = sbox0[i] ^ (key & 0xFF)
        i = i + 1
    ...
    return btoa(result)
```

Now the user SEES the algorithm. They can extract S-boxes and rewrite the cipher in native code without running the VM at all.

OR run it (via exec):
```rust
let mut interp = exec::Interpreter::new(module);
interp.set_hook(Box::new(web_hook));  // from vm-engine-web
interp.add_breakpoint(Breakpoint::OnCall { name: "btoa".into() });
interp.run();
// inspect state at breakpoint to see btoa input = the encoded payload
```

---

## Research Required

Areas that need study before implementation. Each requires reading existing work (papers, open-source decompilers) and testing on the 6 known VMs.

| Area | Module | What to Study | Why It Matters |
| --- | --- | --- | --- |
| Structural analysis algorithm | `structure/region.rs` | Ghidra "dream" decompiler, RetDec pattern-based approach, "No More Gotos" paper (Yakdan et al. 2015) | This determines whether output is readable structured code or a flat mess with gotos |
| Expression reconstruction | `structure/expr.rs` | How Ghidra and RetDec inline temporary variables into compound expressions | Turns 5 IR lines into `(sbox[0] & 0xFF) ^ key` |
| Switch detection | `structure/region.rs` | Jump table patterns, comparison cascade patterns in real VM bytecode | Multiple VMs use switch-like dispatch internally |
| SSA construction | `ir/` (optional pass) | Cytron et al. 1991 — phi node insertion via dominator frontiers | Clean SSA enables better data flow analysis and expression reconstruction |
| Irreducible control flow | `graph/loops.rs` | Node splitting algorithms for CFGs without clean loop headers | Some VMs may produce irreducible flow from computed jumps |
| Stack-to-SSA conversion | decoder pattern | How to convert stack-based VM operations to SSA variables during decoding | FunCaptcha, Shape F5, DataDome PLV3 are all stack-based |

---

## File Map

```text
vm-engine-core/src/
├── lib.rs                          re-exports
├── value/
│   ├── mod.rs                      Value enum, constructors, Display, PartialEq
│   ├── coerce.rs                   to_boolean, to_number, to_int32, to_uint32, to_string
│   └── ops.rs                      BinaryOp, UnaryOp, binary(), unary()
├── ir/
│   ├── mod.rs                      Module, Function, Block, Instruction, Var, FuncId, BlockId
│   ├── opcode.rs                   OpCode (47 operations), Terminator
│   ├── operand.rs                  Operand, SourceLoc
│   ├── builder.rs                  IrBuilder, FunctionBuilder, BlockBuilder
│   ├── validate.rs                 well-formedness checks
│   └── display.rs                  pretty-print IR text
├── graph/
│   ├── mod.rs                      Cfg, build_cfg()
│   ├── block.rs                    CfgBlock, EdgeKind
│   ├── dominator.rs                DominatorTree, compute_dominators(), compute_post_dominators()
│   ├── loops.rs                    LoopInfo, detect_loops()
│   └── callgraph.rs               CallGraph, build_call_graph()
├── structure/
│   ├── mod.rs                      recover() entry point
│   ├── ast.rs                      Stmt, Expr, SwitchCase
│   ├── region.rs                   loop, if/else, switch pattern detection
│   ├── expr.rs                     expression reconstruction
│   └── simplify.rs                 constant folding, dead code removal
└── exec/
    ├── mod.rs                      Interpreter: run(), step(), step_block()
    ├── state.rs                    State, Cursor, CallFrame
    ├── heap.rs                     Heap, Object, CallableKind
    ├── scope.rs                    ScopeChain, Scope
    ├── breakpoint.rs               Breakpoint conditions
    ├── trace.rs                    TraceEvent, TraceRecorder, TraceFilter
    └── hooks.rs                    Hook trait, NullHook
```

5 modules. 22 files. No file exceeds one concern.

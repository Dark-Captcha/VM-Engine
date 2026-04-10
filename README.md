# VM-Engine

Universal bytecode VM analysis framework. Converts any VM bytecode into one structured IR format, then analyzes, recovers control flow, and optionally executes it.

Built for reversing anti-bot VMs (FunCaptcha, Cloudflare, Kasada, Shape F5, DataDome, Incapsula) — but works on any stack-based or register-based bytecode VM.

## Architecture

```
Raw bytecode → [Your decoder] → IR → analysis / decompilation / execution
```

Seven modules, strict layering, no cycles:

| Module | Job |
|--------|-----|
| `value` | JS-compatible type system — Value enum, ECMAScript operators and coercion |
| `ir` | Universal instruction set — Module → Function → Block → Instruction |
| `exec` | IR interpreter with hooks, tracing, breakpoints |
| `web` | Browser environment — Math, JSON, Date, navigator, document, crypto |
| `graph` | CFG, dominator trees, loop detection, call graph |
| `structure` | Decompilation — recovers if/else, while, expressions from CFG |
| `disasm` | Output — IR listing, pseudo-JS, Graphviz DOT, summary stats |

## Quick Start

```rust
use vm_engine::ir::builder::IrBuilder;
use vm_engine::ir::opcode::OpCode;
use vm_engine::ir::operand::Operand;
use vm_engine::value::Value;
use vm_engine::graph;
use vm_engine::disasm;

// Build IR from your decoder
let mut builder = IrBuilder::new();
builder.begin_function("main");
builder.create_and_switch("entry");
let a = builder.const_number(185.0);
let b = builder.const_number(171.0);
let _ = builder.emit(OpCode::BitXor, vec![Operand::Var(a), Operand::Var(b)]);
builder.halt();
builder.end_function();
let module = builder.build();

// Analyze
let func = &module.functions[0];
let cfg = graph::build_cfg(func);
let doms = graph::dominator::compute_dominators(&cfg);
let loops = graph::loops::detect_loops(&cfg, &doms);

// Output
println!("{}", disasm::disasm_structured(&module));
println!("{}", disasm::disasm_summary(&module));
```

## Writing a Decoder

Each VM needs ~300-500 lines of decoder code. The decoder reads raw bytecode and emits IR using `IrBuilder`:

```rust
fn decode(bytecode: &[u8]) -> vm_engine::ir::Module {
    let mut builder = IrBuilder::new();
    builder.begin_function("main");
    builder.create_and_switch("entry");

    let mut pc = 0;
    while pc < bytecode.len() {
        let op = bytecode[pc]; pc += 1;
        match op {
            0x1E => { /* ADD: pop 2, push 1 */ }
            0x42 => { /* PUSH: read typed value */ }
            // ... ~100 more cases
            _ => {}
        }
    }

    builder.end_function();
    builder.build()
}
```

See `examples/plv3/` for a complete DataDome PLV3 decoder (101 opcodes, 141 functions, S-box extraction, cipher, token generation).

## Running with Browser Environment

```rust
use vm_engine::exec::Interpreter;
use vm_engine::web::{self, WebConfig};

let mut interp = Interpreter::new(&module)?;
let global = interp.state.heap.alloc();
interp.state.global_object = Some(global);
web::install_all(&mut interp.state.heap, global, &WebConfig::default());
interp.run()?;
```

## Tests

```bash
cargo test          # 200 lib tests + 4 doc tests
cargo run --example plv3  # DataDome PLV3 proof-of-concept
```

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — full module details, design principles, dependency graph

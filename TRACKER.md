# PLV3 Progress Tracker

> **Updated:** 2026-04-11 (session 2, post-investigation) | **Goal:** Extract obfuscated key names from PLV3 bytecode in pure Rust

---

## Pipeline Status

| Stage | Status | Detail |
|-------|--------|--------|
| Bytecode decode | **DONE** | 101/101 opcodes, 0 unknown. Verified against handlers.json |
| Multi-function decoder | **DONE** | 141 functions + main. Linear pass with MAKE_FUNC+JMP_FWD consumption |
| S-box extraction | **DONE** | 29/29 verified byte-perfect against reference sbox.json |
| Cipher | **DONE** | Fisher-Yates KSA + RC4-variant, generates 203-char base64url tokens |
| Token generation | **DONE** | End-to-end with known keys: sbox0 + cipher_sboxes + keys → valid token |
| Key extraction (IR) | **BLOCKED** | IR interpreter can't model PLV3 stack across block boundaries |
| Key extraction (direct) | **NEAR COMPLETE** | Interpreter reaches HALT (79k instrs), 10+ closures run, **all 8 JSON.stringify values captured correctly**, cipher output still produces Undefined at btoa |
| JS sandbox fallback | **WORKS** | `node fetch_plv3_keys.js` extracts all 8 keys correctly |

---

## 2026-04-11 Session 2 Investigation: The Stack Memory Model Dead End

### Hypothesis (from Session 1)
The `stack[51]` containing Undefined was thought to be caused by PLV3 `pop` semantics:
PLV3's fixed array keeps values after pop (just decrements a pointer), but Rust `Vec::pop`
destroys values. A later push at the same position gets a different value.

**Planned fix:** Logical stack pointer over a never-shrinking Vec.

### Attempted Fix
Implemented `sp: usize` field with:
- `push(v)` writes `stack[sp]` and increments `sp`
- `pop()` decrements `sp` and returns `stack[sp].clone()` (value stays in Vec)
- `set_reg(r, v)` / `set_frame(f, v)` write to `stack[r]` without touching `sp`
- `RETURN` sets `sp = frame.stack_base` (no `truncate`)
- `COLLECT` (opcode 191) reads slice without draining

### Investigation Result: REVERTED

The SP refactor introduced regressions that were **worse** than the original problem:

| Metric | Session 1 (old) | Session 2 SP-refactor | After revert |
|---|---|---|---|
| HALT reached | ✅ PC=123642 | ✅ PC=123642 | ✅ PC=123642 |
| Instructions | 79,033 | 84,738 | 79,033 |
| btoa calls | 8 (Undefined) | 8 (Undefined) | 8 (Undefined) |
| JSON.stringify values correct | **8/8** | 6/8 | **8/8** |

### Trace-level root cause of the regression

Investigation path:
1. Saw `JSON.stringify #2 arg=undef` at sp=6401, should be `Number(46.0)` (client_width)
2. Traced back: `PUSH_REG r=652` at PC=92856 pushed `undef` (from `get_reg(652)`)
3. `stack[652] = undef` — where did that come from?
4. `set_reg(652, undef)` was called at PC=88896 (instr 72268)
5. At PC=88891 (op 202 GET_PROP): popped key="clientWidth", popped obj, called `get_prop(obj, "clientWidth")`
6. **Key finding:** `obj = Number(0)` (not `Value::Object(div)`)
7. `get_prop(Number(0), "clientWidth")` returns `undef` — a number has no properties
8. So STORE_PUSH_IMM (op 111) wrote that `undef` to `set_reg(652, undef)`
9. Later, `PUSH_REG r=652` read back that `undef` as the "client_width" value

**Conclusion:** In the refactored model, the stack position that should contain the
div element object contained `Number(0)` instead. This happened because the SP refactor
changed the **absolute positions** of pushed values.

### Why the positions shifted

The old Vec-based model's `set_reg(r, v)` where `r > stack.len()` did:
```rust
while self.stack.len() <= r { self.stack.push(prng_value); }
self.stack[r] = v;
```
This grew the Vec AND effectively advanced the logical SP to `r+1`. A subsequent `push(x)`
would land at `stack[r+1]`.

The new SP model's `set_reg(r, v)` only grew the Vec (via `ensure_memory`) — SP unchanged.
A subsequent `push(x)` would land at `stack[old_sp]`, NOT at `stack[r+1]`.

Same opcode sequence, different absolute positions. Register reads (which use absolute
indices like `stack[652]`) retrieved different values in the two models.

### Why the old model was "correct"

The old model matched PLV3's JS sandbox output for all 8 JSON.stringify values
(confirmed by running the JS reference side-by-side). Even though the old model's
memory semantics diverge from real PLV3 (Vec::pop destroys vs PLV3 preserves), the
**decoder happened to produce correct outputs** because the absolute positions
aligned with where PLV3 expected values to land.

The new (semantically-cleaner) model broke this alignment and lost 2/8 values.

### Lesson learned

> The Vec-based model is semantically wrong for PLV3 but empirically correct for the
> values we need to extract. The SP-based model is semantically closer to real PLV3
> but empirically wrong because it shifted absolute register positions.
>
> **The real btoa bug is NOT in the stack memory model.** It's somewhere else — most
> likely in a specific opcode implementation, or in the cipher loop's interaction
> with either the register layout or a native function's return value.

---

## Session 1 Progress (still valid)

### Major Bugs Fixed

#### Bug 1: PLV3 initializes arrays with PRNG values (not Undefined) — FIXED
Found in `vm_module.js`:
```javascript
Array.from({length: 114485}, (Q, C) => C >= E && C < E + g ? A.charCodeAt(C - E) : B())
```
Where `B()` is an XorShift PRNG seeded with `6430266`. Uninitialized register reads should return PRNG bytes (0-255), NOT `Undefined`.

**Fix:** Implemented `Plv3Prng` struct matching JS byte-for-byte (verified: 130, 10, 55, 232).
Added `prng_values: [u8; 512]` field; `get_reg()` and `set_reg()` use it as fallback.

#### Bug 2: Tag 98 was incorrectly treated as Undefined — FIXED
The bytecode version uses tag 98 for `null`, not `undefined`. Treating as `Undefined` caused NaN cascades in arithmetic; treating as `null` allows proper 0-coercion in bitwise ops.

**Fix:** `examples/plv3/reader.rs:55` — `98 => Value::Null`

#### Bug 3: new Date() returned Undefined — FIXED
Added `construct_date()`; POS opcode (41) extracts `__date_timestamp__`.

#### Bug 4: Opcode 118 (PUSH_REG_PROP_AND) was wrong — FIXED
Original: single property access. Correct (per handlers.json): two separate pushes —
`push(reg[a]); push(reg[b] & imm)`.

---

## Library-Side Improvements (session 2, all kept)

The library (`src/`) received substantial improvements independent of the PLV3 debugging.
All improvements are strict upgrades and have no regressions.

### Value/Coercion fixes
- `to_number([])` now returns `0` (was `NaN`) — via toString → ToNumber path
- `to_string([1,2,3])` now returns `"1,2,3"` (was `"[object Array]"`)
- `abstract_eq` now handles array/object-to-primitive coercion per spec

### Interpreter fixes (`src/exec/`)
- **Switch terminator**: was missing `previous_block = Some(current_block)` — Phi nodes broken
- **HasProp**: operand order was `(key, obj)`, fixed to `(obj, key)` matching LoadProp/StoreProp
- **CallFrame.result_var**: explicit result-var tracking so Return stores the return value
  in the correct Var without guessing via cursor position
- **Per-instance unhandled-call counter**: was a shared `static` that bled across instances
- **`set_strict_calls(bool)`**: new API to surface unresolved Call instructions as errors
- Cursor no longer double-advances past a Call that entered its callee

### IR validator enhancements (`src/ir/validate.rs`)
- Checks for undefined FuncId references
- Checks for reachable blocks with `Terminator::Unreachable` (unset terminator)
- Checks for duplicate switch case values
- Validates opcode operand arity
- Added `IrBuilder::build_validated()` — opt-in validation at build time

### Web environment (`src/web/`)
- **JSON.parse** rewritten as full recursive parser (objects, arrays, \uXXXX escapes)
- Added `addEventListener`/`removeEventListener`/`dispatchEvent` stubs
- Added `setTimeout`/`setInterval`/`setImmediate`/`clearTimeout`/`clearInterval`
- Added `Symbol`, `Promise`, `Object.keys/values`, `Array.isArray` stubs
- Added navigator: `permissions`, `geolocation`, `mediaDevices`, `plugins`, `mimeTypes`,
  `serviceWorker`, `credentials`, `deviceMemory`
- Added document: `head`, `URL`, `domain`, `forms`, `images`, `links`, `getElementById`,
  `getElementsByClassName`, `getElementsByTagName`, `addEventListener`
- `String.fromCharCode` now truncates to u16 (spec-compliant); `fromCodePoint` uses full u32
- **Date.now / performance.now UNCHANGED** — kept `current + tick` semantics (PLV3-compat)

### Graph analysis (`src/graph/`)
- Added `Cfg::reachable_blocks()` and `Cfg::unreachable_blocks()`
- Added `DominatorTree::is_reachable()`
- Dominator-verified natural loop bodies (prevents over-inclusion)
- Call graph now tracks indirect calls via `IndirectCallSite`

### Structure recovery (`src/structure/`)
- **Switch recovery** no longer emits phantom `if (...)` conditions
- Post-dominator merge-point detection for switch cases

### Test coverage
- Library: 200 → **221** passing tests (+21, 0 failures)
- 4 doc tests still pass
- All examples build and run

---

## Remaining Issue: btoa still receives Undefined

**Symptoms (unchanged from session 1):**
1. ✅ 8 JSON.stringify calls fire with correct fingerprint values
2. ❌ Cipher (KSA + RC4) runs BETWEEN stringify and btoa — output is Undefined
3. ❌ 8 btoa calls each get Undefined

**Hypothesis spaces to explore next session:**

### A. Cipher loop bug (most likely)
The cipher runs between stringify and btoa. It's RC4-variant with KSA. If any opcode
in the cipher implementation is wrong, the output will be Undefined/NaN.
- Start: trace the first NaN that appears after JSON.stringify #1 (timestamp).
- The old FIRST-NAN trace showed NaN at PC=21526 but that was during S-box init, not
  cipher output.
- Need to trace where the cipher-produced value ends up, and compare to JS reference.

### B. Specific opcode with wrong semantics
Opcodes to verify against handlers.json:
- `117` (OR via `let imm = self.read_typed_value(); let r = self.pop(); self.push(ops::binary(BinaryOp::BitOr, &r, &imm));`)
- `30`, `29`, `207` — various push variants  
- `44`, `245`, `84` — property sets from registers
- `212` — fused XOR + store + push2
- `45`, `111`, `151`, `10`, `251` — fused store+pop+push variants

### C. Native function return value mismatch
- `JSON.stringify` — my implementation returns "null" for undefined, should match JS
- `btoa` — takes a string, may fail on Number input  
- `Math.random` — uses LCG, but PLV3 may depend on V8's xorshift128+ specifically
- `String.fromCharCode` — fixed to truncate to u16 this session (may affect cipher)

### D. Closure capture / frame setup mismatch
- `call_plv3_closure` frame layout may differ from PLV3 handler 55 in subtle ways
- Captures: are they all being pushed at the right positions?
- Param padding with Undefined vs PRNG — `Value::Undefined` is the documented PLV3 behavior

---

## 8 JSON.stringify Inputs Verified (session 2, post-revert)

These match the 8 expected obfuscated key names (identical to session 1):
```
1. Number(1700000000034.0)  → timestamp
2. String("/")              → pathname
3. Number(46.0)             → client_width
4. Number(1700000000051.0)  → elapsed
5. Number(1002.0)           → perf_now
6. Bool(true)               → is_secure
7. Bool(false)              → webdriver
8. Number(0.48995803...)    → random
```

### HALT Details
- PC at HALT: 123642
- Instructions: 79,033
- Stack depth: 6,317
- btoa calls: 8 (all receiving Undefined)
- Total native calls: 65

---

## Known Keys (this bytecode rotation)

```
timestamp    → "o4zbWU"
pathname     → "P6qv3g"
client_width → "IMd3AC"
elapsed      → "yKUvF2"
perf_now     → "mR8kKc"
is_secure    → "oezAbp"
webdriver    → "J1nt2L"
random       → "KJg3g6"
```

---

## PLV3 Bytecode Facts

| Fact | Value |
|------|-------|
| Bytecode size | 123,643 bytes |
| Total opcodes | 101 (all decoded, 0 unknown) |
| Functions | 141 (via MAKE_FUNC) + 1 main |
| S-boxes | 29 (256-byte Uint8Arrays) |
| Main code pre-functions | PC 0 — 16,560 |
| Function bodies | PC 16,566 — 118,491 |
| Main code post-functions | PC 118,491 — 123,643 |

---

## Key Discoveries

### 1. PLV3 VM Array Initialization Pattern
```javascript
const B = function (A) {
    const B = F();  // XorShift PRNG
    return Array.from({length: 114485}, (Q, C) =>
        C >= E && C < E + g ? A.charCodeAt(C - E) : B()
    );
}(atob(A));
```
Array positions OUTSIDE the bytecode region (0..3833 and 108256..) are filled with PRNG bytes.

### 2. PLV3 XorShift PRNG Semantics
```javascript
let A = 6430266, B = 0;
return function () {
    return 3 & B++ || (A ^= A << 13, A ^= A >> 17, A ^= A << 5),
           (A >> ((3 & B) << 3)) % 256;
};
```
First 10 values: 130, 10, 55, 232, 153, 161, 141, 35, 234, 62.

### 3. Opcode 118 is Two Separate Pushes (Not Property Access)
Handler: `A[w++] = A[a+e], A[w++] = A[a+s] & Q`

### 4. PLV3 VM Architecture (from vm_module.js)
```
Constants:
  B = 3267          (base)
  E = 3833          (bytecode base in array)
  g = 104423        (bytecode length)
  C = E + g = 108256 (register/stack base)
  c = 3358          (IP register index)
  w = 3311          (SP register index)
  D = B + 2 = 3269  (frame pointer register)
```

### 5. Session 2 Discovery: Stack Memory Model
PLV3 `pop` is a pointer decrement — values below SP are preserved in memory. But the
Vec-based interpreter's implementation happens to produce correct PLV3 outputs because
the register-layout math works out by accident. A semantically-cleaner "logical SP
over never-shrinking Vec" model is mathematically equivalent to real PLV3 but breaks
the interpreter because absolute register positions differ. **DO NOT revisit this
refactor without FIRST identifying the real btoa bug.**

### 6. PLV3 Handler Variable Mapping
From `handlers.json` + `vm_module.js`:
- `A[o]` → stack pointer register (w = 3311 in vm_module)
- `A[E]` → frame base pointer
- `A[D]` → frame pointer (used for return address)
- `A[a + X]` → register X (a = C = 108256, the register base)
- `B()` → read u16 operand
- `c()` → read typed value
- `w(X)` → push register X
- `C(X)` → store TOS to register X (no pop)
- `U()` → pop
- `u()` → next instruction

---

## Key Files

| File | Purpose |
|------|---------|
| `examples/plv3/decoder.rs` | Bytecode → IR decoder (101 opcodes) |
| `examples/plv3/interpreter.rs` | **Direct interpreter (Vec-based, reverted from SP refactor)** |
| `examples/plv3/reader.rs` | PLV3 typed value reader — tag 98 is Null |
| `examples/plv3/funcmap.rs` | MAKE_FUNC boundary scanner (141 functions) |
| `examples/plv3/extract.rs` | S-box extraction from IR constant patterns |
| `examples/plv3/cipher.rs` | Full Rust cipher: KSA + RC4-variant + base64url |
| `examples/plv3/execute.rs` | Key extraction harness (Hook for btoa/stringify) |
| `examples/plv3/main.rs` | CLI — detailed call tracing |
| `src/exec/mod.rs` | IR interpreter — **Switch Phi fix, HasProp order fix, strict_calls** |
| `src/ir/validate.rs` | **Enhanced: arity, reachability, func refs, switch dupes** |
| `src/value/coerce.rs` | **Array ToNumber/ToString fixed (no more NaN cascade)** |
| `src/value/ops.rs` | **Abstract equality handles array/object → primitive** |
| `src/web/globals.rs` | **Event listeners, timers, Symbol, Promise, Object, Array** |
| `src/web/navigator.rs` | **permissions, geolocation, mediaDevices, plugins** |
| `src/web/document.rs` | **head, URL, forms, getElementById, addEventListener** |
| `src/web/json.rs` | **Full JSON.parse (objects, arrays, escapes)** |
| `src/web/timing.rs` | Date.now / performance.now — unchanged (PLV3-compat) |
| `src/graph/callgraph.rs` | **Tracks indirect calls via IndirectCallSite** |
| `src/graph/loops.rs` | **Dominator-verified natural loop bodies** |
| `src/structure/region.rs` | **Switch recovery no phantom conditions** |

---

## Next Session Action Plan

1. **Don't touch the stack memory model.** It's Vec-based and that's what works for PLV3.
2. **Focus on the cipher loop.** The bug is between JSON.stringify and btoa — somewhere
   in the RC4-variant permutation and base64url encoding.
3. **First step:** Trace the exact byte sequence that goes INTO the cipher for one
   fingerprint (e.g., timestamp value 1700000000034). Compare against JS reference
   running side-by-side. Find the first opcode where outputs diverge.
4. **Tools to use:**
   - Existing `[native-call-pre]` trace shows JSON.stringify inputs are correct
   - Need to add a cipher-side trace that shows intermediate values between
     JSON.stringify and btoa
   - The btoa arg is `undef` — so somewhere between stringify and btoa, the cipher
     output becomes Undefined. Find the opcode that produces that Undefined.
5. **Avoid rabbit holes:**
   - No more stack memory model experiments
   - No more spec-compliance changes to Date.now / timing (PLV3 relies on exact values)
   - Don't refactor opcode handlers without first confirming they're wrong via trace

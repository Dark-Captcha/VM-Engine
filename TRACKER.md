# PLV3 Progress Tracker

> **Updated:** 2026-04-10 | **Goal:** Extract obfuscated key names from PLV3 bytecode in pure Rust

---

## Pipeline Status

| Stage | Status | Detail |
|-------|--------|--------|
| Bytecode decode | **DONE** | 101/101 opcodes, 0 unknown. Verified against handlers.json |
| Multi-function decoder | **DONE** | 141 functions + main. Linear pass with MAKE_FUNC+JMP_FWD consumption |
| S-box extraction | **DONE** | 29/29 verified byte-perfect against reference sbox.json |
| Cipher | **DONE** | Fisher-Yates KSA + RC4-variant, generates 203-char base64url tokens |
| Token generation | **DONE** | End-to-end with known keys: sbox0 + cipher_sboxes + keys → valid token |
| Key extraction | **BLOCKED** | IR interpreter can't model PLV3 stack across block boundaries |
| JS sandbox fallback | **WORKS** | `node fetch_plv3_keys.js` extracts all 8 keys correctly |

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

Extracted via JS sandbox (zero-sbox trick). Keys are **computed at runtime** via S-box lookups + String.fromCharCode — NOT stored as string constants in the bytecode.

---

## Root Cause: Why IR Interpreter Fails

The PLV3 VM is a **stack machine**. The VM stack persists across the entire execution. Our decoder converts to **block-based SSA IR** which clears the symbolic stack at block boundaries.

**The specific failure chain:**

1. S-box creation (`new window.Uint8Array([256 values])`) leaves 29 results on the PLV3 stack (not stored to registers yet)
2. 141 MAKE_FUNC opcodes push closures on top → stack depth ~170
3. Between-function code pushes/pops but closures + S-boxes stay buried
4. Eventually a branch terminates the entry block → sym_stack cleared → all 170+ values lost
5. Later code loads function refs from registers via `LoadScope("reg_X")` → **Undefined** because the registers were populated from stack values that no longer exist in the IR

**Why this is fundamental (not a bug to fix):**
- The PLV3 VM's stack carries values across branches. SSA IR can't model this without PHI nodes.
- The linear decoder tried: (a) clearing stack at boundaries (loses values), (b) not clearing (wrong values at merge points), (c) spilling to scope vars (positions shift). None work correctly for 170+ accumulated stack items across 350+ branches.

**The correct solution:** Direct bytecode interpreter that maintains a real runtime stack (like the JS VM does). The IR stays for static analysis (S-box extraction, decompilation) but NOT for execution.

---

## Bugs Fixed This Session

### 1. JMP_FALSY_KEEP / JMP_TRUTHY_KEEP missing terminators
- **Opcodes:** 169, 89
- **Bug:** `if let Some(&cond_var) = sym_stack.last()` guard silently skipped `branch_if` when stack empty at block boundaries. Block got default `Unreachable` terminator.
- **Fix:** Use `sym_stack.last().copied().unwrap_or_else(|| stack_pop(...))` — always emit the branch.
- **Impact:** Interpreter went from 22k to 147k instructions.

### 2. Opcode 150 operand size mismatch in skip_operands
- **Bug:** `funcmap.rs` and `find_block_starts` treated opcode 150 (PUSH_REG_IMM) as `2g` (4 bytes) but it's actually `1g+1i` (u16 + typed_value). Desynced ALL jump target detection after the first opcode 150.
- **Fix:** Moved 150 from `2g` group to `1g+1i` group in both `funcmap::skip_operands` and `find_block_starts`.

### 3. MAKE_FUNC+JMP_FWD not consumed in pre-scan
- **Bug:** `find_block_starts_in_region_skipping` saw JMP_FWD after MAKE_FUNC and created block boundaries at function body_end PCs. The decoder consumed the JMP_FWD but the pre-scan didn't, causing block splits that broke the entry block.
- **Fix:** Added MAKE_FUNC (opcode 55) handling to the pre-scan — consume header + JMP_FWD, don't add targets.

### 4. NewArray created immutable Value::Array
- **Bug:** `OpCode::NewArray` returned `Value::Array(Vec::new())`. `StoreProp` only handled `Value::Object`, so COLLECT arrays were always empty.
- **Fix:** NewArray now allocates on heap (`Value::Object(oid)`) so StoreProp can populate elements.

### 5. Missing web environment features
- Added `Uint8Array` constructor (heap-allocated, handles Object arrays from COLLECT)
- Added `__bind__` and `new` meta-call handling in interpreter Call dispatch
- Added `document.body` with `appendChild`/`removeChild` stubs

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
| First MAKE_FUNC | PC 16,560 (func_16566, 13 params) |
| Last function body_end | PC 118,491 |

**Bytecode layout:**
```
[S-box init: 29 Uint8Arrays created, NOT stored to regs yet]
[MAKE_FUNC #0 + JMP_FWD → body_end]
[between-func code: r22[149] = r12[180]]
[MAKE_FUNC #1 + JMP_FWD → body_end]
[between-func code: 2183 bytes of computation + stores]
... 139 more MAKE_FUNCs ...
[post-function code: computation, JSON construction, cipher, btoa]
[HALT]
```

**Stack at first MAKE_FUNC (PC 16560):** depth=29 (29 S-box Uint8Array results, not yet stored to registers)

**Key names are computed at runtime** — built character by character via:
1. S-box lookups (`sbox[i & 255]`)
2. XOR/arithmetic transformations
3. `String.fromCharCode(computed_values)`
4. Concatenation → 6-char alphanumeric key names

---

## Opcode Verification

All 48 critical opcodes verified correct against `handlers.json` ground truth:
- Operand sizes: all match (including the fixed opcode 150)
- Stack effects: all match
- Operand order: all match

**Typed value tags (reader.rs):**
| Tag | Type | Size |
|-----|------|------|
| 128+ | small int (tag & 0x7F) | 1 byte |
| 38 | true | 1 byte |
| 107 | false | 1 byte |
| 98 | undefined | 1 byte |
| 63 | XOR string (key starts at 27) | variable |
| 47 | UTF8 string (key starts at 61) | variable |
| 61 | float64 | 9 bytes |
| 122 | int32 | 5 bytes |
| 68 | int24 | 4 bytes |
| 123 | int16 | 3 bytes |
| 53 | int8 | 2 bytes |

---

## Key Files

| File | Purpose |
|------|---------|
| `examples/plv3-proof/src/decoder.rs` | Linear decoder: MAKE_FUNC+JMP_FWD consumption, 101 opcodes |
| `examples/plv3-proof/src/reader.rs` | PLV3 typed value reader (XOR strings, variable-length ints) |
| `examples/plv3-proof/src/funcmap.rs` | MAKE_FUNC boundary scanner (141 functions) |
| `examples/plv3-proof/src/extract.rs` | S-box extraction from IR constant patterns |
| `examples/plv3-proof/src/cipher.rs` | Full Rust cipher: KSA + RC4-variant + base64url |
| `examples/plv3-proof/src/execute.rs` | Key extraction harness (Hook for btoa/stringify) |
| `crates/vm-engine-core/src/exec/mod.rs` | IR interpreter with __bind__/new handling |
| `crates/vm-engine-web/src/globals.rs` | Uint8Array + global functions |
| `crates/vm-engine-web/src/document.rs` | document.body stubs |
| `/home/gnusocute/Documents/Dark-Captcha_old/datadome/fetch_plv3_keys.js` | JS sandbox key extractor (WORKING) |
| `/home/gnusocute/Documents/Dark-Captcha_old/datadome/plv3-vm/source/handlers.json` | Ground truth opcode handlers |
| `/home/gnusocute/Documents/Dark-Captcha_old/datadome/plv3-vm/source/bytecode.bin` | Real PLV3 bytecode |
| `/home/gnusocute/Documents/Dark-Captcha_old/datadome/plv3-vm/out/sbox.json` | Reference S-boxes for verification |

---

## Next Steps

1. **Restructure** — Single crate with `src/targets/plv3/` (no more 3 separate crates)
2. **Build direct PLV3 interpreter** (`targets/plv3/interpreter.rs`) — stack-based, no IR conversion
3. **Key extraction via direct interpreter** — run with web env, hook btoa → extract JSON keys
4. **Remove IR from execution path** — IR stays for analysis/decompilation only

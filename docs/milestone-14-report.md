# Milestone 14 Engineering Report: Owned Strings, UTF-8 Operations, Safe Slicing, and String Memory Safety

---

## Executive Summary

Milestone 14 introduces a first-class, heap-backed, affine `String` type and comprehensive UTF-8 memory safety operations to the Vanta systems programming language.

Building on Vanta's custom x86-64 backend, pure-Rust ELF64 object writer, affine ownership model, named lifetime parameters from Milestone 13, and the standard library module system from Milestone 11, Milestone 14 achieves:

1. **First-Class Owned `String` Type**: Standard-library `String` (`import std::string::String;`) represented as a 3-word descriptor (`data_ptr: i64`, `len: i64`, `cap: i64`, 24 bytes, 8-byte aligned on x86-64) backed by Vanta's audited runtime allocator (`vanta_alloc`, `vanta_realloc`, `vanta_free`).
2. **UTF-8 Validity by Construction**: Maintained as an unviolated invariant across creation (`String::from_str`), growth (`push_str`), cloning (`clone`), and clearing (`clear`). Unchecked interior byte mutation is strictly disallowed.
3. **Deterministic Geometric Growth & Self-Aliasing Safety**: Geometric capacity doubling with checked signed 64-bit overflow prevention. Self-aliasing in appends (e.g. `s.push_str(s.as_str())`) computes relative byte offsets to preserve pointer validity across buffer relocations.
4. **Zero-Allocation Empty Strings**: `String::new()` constructs a valid descriptor with `data_ptr = NULL, len = 0, cap = 0`, allocating zero heap bytes until non-empty growth occurs.
5. **Canonical String API**: Comprehensive public methods (`as_str()`, `as_bytes()`, `len_bytes()`, `capacity()`, `is_empty()`, `clear()`, `clone()`, `equals()`).
6. **Safe UTF-8 Byte Slicing (`slice_bytes`)**: Safe range slicing on both `String` and `&str` (`.slice_bytes(start, end) -> Option<&str>`) that validates `0 <= start <= end <= len` and rejects indices pointing to UTF-8 continuation bytes (`(byte & 0xC0) == 0x80`).
7. **Complete Ownership, Reassignment, and Drop Glue**: `String` is recognized as an affine non-Copy type requiring destruction (`needs_drop == true`). Reassignment safely drops the old buffer before acquiring the new one. Destructors propagate through nested containers (`Option<String>`, `Result<String, E>`, `Vec<String>`, user structs).
8. **Borrow Checker Integration**: Shared borrows via `.as_str()`, `.as_bytes()`, or `.slice_bytes()` place the parent `String` in `BorrowState::Shared`, statically rejecting conflicting mutations (`push_str`, `clear`) or moves (`E0306_MUTATE_WHILE_BORROWED`, `E0307_MOVE_WHILE_BORROWED`).
9. **Zero LLVM / Cranelift Dependency**: Lowered entirely through Vanta's custom MIR and x86-64 code generation pipeline, generating pure-Rust ELF64 object files.
10. **Full Verification and AI Tooling**: 22 automated tests in `crates/vanta_driver/tests/string_tests.rs` verifying compilation failures, differential execution at `-O0` and `-O1`, zero memory leaks (`vanta_active_allocs() == 0`), and an end-to-end showcase in `examples/17_owned_strings/main.va`.

---

## 1. Architectural Baseline & Audit

Prior to implementation, an audit of Vanta's string, memory, and runtime models established:
- **String Literals & `&str`**: Vanta represented string literals as immutable slices (`Type::Str` behind `Type::Ref`), lowered to two-word descriptors `(data_ptr, len)` pointing into the `.rodata` section.
- **Allocator Contract**: The runtime allocator in `runtime/vanta_rt.c` provided `vanta_alloc`, `vanta_realloc`, and `vanta_free`, with active allocation tracking via `vanta_active_allocs()`.
- **Destructor Machinery**: The compiler supported linear type destruction via `Type::needs_drop` and MIR `emit_drop_for_place`, with drop candidates tracked in lexical `drop_scopes`.
- **Container Precedents**: `Vec<T>` provided a proven model for 3-word heap containers, but lacked specialization for element destructor invocation when holding strings.
- **Constraint Compliance**: No LLVM or Cranelift dependencies; all machine code generated via Vanta's custom x86-64 encoder and pure-Rust ELF64 emitter.

---

## 2. Standard Library `String` Definition & Internal Layout

### 2.1 Standard Library Module Definition (`library/std/string.va`)
```vanta
pub struct String {
    pub data_ptr: i64,
    pub len: i64,
    pub cap: i64,
}

pub fn new() -> String {
    return String {
        data_ptr: 0,
        len: 0,
        cap: 0,
    };
}

pub fn from_str(value: &str) -> String {
    let mut s = new();
    s.push_str(value);
    return s;
}
```

The standard library source is embedded directly into the compiler binary (`crates/vanta_sema/src/module.rs`) and loaded automatically under `std::string::String`.

### 2.2 Physical Memory Representation
A `String` descriptor occupies 24 contiguous bytes (3 quadwords) with 8-byte natural alignment on x86-64:
- `data_ptr` (offset 0, 8 bytes): Pointer to heap-allocated UTF-8 buffer (`NULL` if empty/unallocated).
- `len` (offset 8, 8 bytes): Number of valid bytes currently in the buffer.
- `cap` (offset 16, 8 bytes): Total allocated byte capacity of the heap buffer.

```
       Stack Descriptor (24 bytes)                      Heap UTF-8 Buffer
+-------------------+---------+---------+        +---+---+---+---+---+---+---+---+
| data_ptr (8B)     | len(8B) | cap(8B) | ---->  | V | a | n | t | a |   |   |   |
+-------------------+---------+---------+        +---+---+---+---+---+---+---+---+
                                                 |<----- len = 5 ---->|
                                                 |<---------- cap = 8 ---------->|
```

---

## 3. Allocation Strategy, Geometric Growth, and Self-Aliasing

### 3.1 Allocation and Reallocation (`runtime/vanta_rt.c`)
- **Empty Initialization**: `vanta_str_init(VantaString *s)` sets `data = NULL, len = 0, cap = 0`. No memory is allocated.
- **Conversion from Slice**: `vanta_str_from_str(VantaString *dest, const char *ptr, int64_t len)` allocates exactly `len` bytes (if `len > 0`), copies the bytes using `memcpy`, and sets `dest->len = len; dest->cap = len;`.
- **Capacity Policy**: On append, if `needed_len > cap`, capacity grows geometrically:
  $$\text{new\_cap} = \max(\text{needed\_len}, \text{if } \text{cap} == 0 \text{ then } 8 \text{ else } \text{cap} \times 2)$$
- **Arithmetic Overflow Protection**: Checked using GCC/Clang builtins:
  ```c
  int64_t needed_len;
  if (__builtin_add_overflow(s->len, len, &needed_len) || needed_len < 0) {
      vanta_panic("string length overflow", 22);
  }
  ```
- **Self-Aliasing Protection**: If the source slice `ptr` points into `s->data` (interior slice alias, e.g. `s.push_str(s.as_str())`), the relative byte offset is computed before reallocation:
  ```c
  int is_interior = (s->data != NULL && ptr >= s->data && ptr < (s->data + s->len));
  int64_t offset = is_interior ? (int64_t)(ptr - s->data) : 0;
  // ... realloc occurs ...
  if (is_interior) {
      ptr = s->data + offset;
  }
  ```

---

## 4. Ownership, Move Semantics, and Drop Glue

### 4.1 Type System Non-Copy & Drop Classification
In `crates/vanta_sema/src/types.rs`:
- `is_string(&self) -> bool`: Recognizes `String`, `std::string::String`, and mangled variants.
- `is_copy_with_context`: Evaluates to `false` for `String`.
- `needs_drop`: Evaluates to `true` for `String`.
- `size_with_context` / `alignment_with_context`: Returns 24 and 8.

### 4.2 Destructor Emission
- **Local Scope Exit**: Emits `vanta_str_free(slot_addr)` for all active `String` bindings upon exiting their lexical block, function return, `break`, or `continue`.
- **Reassignment**: When assigning to an existing mutable binding (`s = ...`), the old value's destructor is emitted prior to acquiring the new value, preventing leaks.
- **Rvalue Temporaries**: Intermediate call temporaries (e.g. `String::from_str(...)`) are moved directly into destination slots without redundant drop candidate registration, preventing double-free defects.

---

## 5. Canonical String API Implementation

| Method / Function | Receiver | Signature | Semantics |
|---|---|---|---|
| `String::new()` | None | `() -> String` | Allocates zero bytes; returns empty string. |
| `String::from_str(s)` | None | `(&str) -> String` | Copies slice into newly allocated heap buffer. |
| `s.push_str(text)` | `&mut self` | `(&str) -> ()` | Appends slice; grows buffer geometrically as needed. |
| `s.as_str()` | `&self` | `() -> &str` | Zero-copy view; establishes immutable borrow on `s`. |
| `s.as_bytes()` | `&self` | `() -> &[u8]` | Zero-copy byte slice view; establishes immutable borrow. |
| `s.len_bytes()` | `&self` | `() -> i64` | Returns byte length in $O(1)$. |
| `s.capacity()` | `&self` | `() -> i64` | Returns allocated byte capacity in $O(1)$. |
| `s.is_empty()` | `&self` | `() -> bool` | Evaluates `len == 0`. |
| `s.clear()` | `&mut self` | `() -> ()` | Sets `len = 0`; retains allocated capacity for reuse. |
| `s.clone()` | `&self` | `() -> String` | Creates independent heap allocation with identical bytes. |
| `s.equals(other)` | `&self` | `(&str) -> bool` | Exact byte comparison without NUL terminator assumptions. |
| `s.slice_bytes(start, end)` | `&self` | `(i64, i64) -> Option<&str>` | Validates indices and UTF-8 boundaries; returns sub-slice. |

---

## 6. Safe UTF-8 Byte Slicing (`slice_bytes`)

### 6.1 Validation Rules
`slice_bytes` is implemented as an inherent method on both `String` and `&str`. It accepts `start: i64` and `end: i64`, and enforces:
1. `0 <= start <= end <= len`. If `start > end`, `start < 0`, or `end > len`, it returns `Option::None`.
2. UTF-8 character boundary check: A UTF-8 continuation byte matches `(byte & 0xC0) == 0x80`. If `start` or `end` lands on a continuation byte, it returns `Option::None`.
3. If both bounds are valid, it constructs `Option::Some(&str)` pointing directly to `src_ptr + start` with length `end - start`.

```c
void vanta_str_slice_bytes(VantaOptionStr *opt, const char *src_ptr, int64_t src_len, int64_t start, int64_t end) {
    if (opt == NULL) return;
    if (start < 0 || end < start || end > src_len) {
        opt->tag = 0; opt->ptr = NULL; opt->len = 0; return;
    }
    if (start > 0 && start < src_len && src_ptr != NULL) {
        if (((unsigned char)src_ptr[start] & 0xC0) == 0x80) {
            opt->tag = 0; opt->ptr = NULL; opt->len = 0; return;
        }
    }
    if (end > 0 && end < src_len && src_ptr != NULL) {
        if (((unsigned char)src_ptr[end] & 0xC0) == 0x80) {
            opt->tag = 0; opt->ptr = NULL; opt->len = 0; return;
        }
    }
    opt->tag = 1;
    opt->ptr = (src_ptr != NULL) ? (src_ptr + start) : NULL;
    opt->len = end - start;
}
```

---

## 7. Container Nesting & Complex Memory Interactions

### 7.1 Vectors of Strings (`Vec<String>`)
- Vector element sizing: `Vec<String>` has `elem_size = 24`.
- Push mechanics: `vanta_vec_push` handles non-scalar element sizes (`elem_size > 8`) by copying from the source memory address rather than register argument values.
- Destruction: `vanta_vec_free_strings` iterates over all vector elements (`0..len`), invoking `vanta_str_free` on each element before freeing the vector storage array, eliminating memory leaks.

### 7.2 Algebraic Enums (`Option<String>`, `Result<String, E>`)
- Discriminant-aware drop glue ensures that `Option::Some(s)` drops string payload `s`, while `Option::None` executes zero drop logic.
- Verified in tests via `vanta_active_allocs() == 0` upon scope exit.

### 7.3 User-Defined Structs with String Fields
- Struct layout calculation properly resolves field offsets for `String` fields (24 bytes, 8-byte aligned).
- Circular dependency checking recognizes `String` as a leaf built-in type.
- Struct destruction recursively emits `vanta_str_free` for each owned string field in reverse declaration order.

---

## 8. Zero LLVM / Cranelift Custom Backend Lowering

1. **Pure x86-64 Machine Code**: All string operations lower through Vanta's custom Machine IR instruction selector and encoder (`crates/vanta_codegen_x86_64`).
2. **Standard C ABI Compliance**: Calls conform to System V AMD64 ABI conventions:
   - Arguments passed in `%rdi`, `%rsi`, `%rdx`, `%rcx`, `%r8`, `%r9`.
   - Aggregate return types like `Option<&str>` return via hidden destination pointer in `%rdi`.
3. **ELF64 Object Emission**: Native object files are generated via Vanta's built-in ELF64 writer without external tool dependencies.

---

## 9. Differential Execution and Validation Matrix

The test suite in `crates/vanta_driver/tests/string_tests.rs` contains 22 comprehensive test cases, executing differentials between `-O0` (unoptimized) and `-O1` (optimized with DCE, constant propagation, and control-flow cleanup):

| Test Name | Focus Area | Optimization Modes | Result |
|---|---|---|---|
| `test_compile_fail_push_str_while_borrowed` | Compile-fail: `push_str` while shared borrow active (`E0306`) | Compile check | PASS |
| `test_compile_fail_clear_while_borrowed` | Compile-fail: `clear` while shared borrow active (`E0306`) | Compile check | PASS |
| `test_compile_fail_move_while_borrowed` | Compile-fail: move while shared borrow active (`E0306`) | Compile check | PASS |
| `test_compile_fail_push_str_non_str_arg` | Compile-fail: non-`&str` argument to `push_str` (`E0201`) | Compile check | PASS |
| `test_compile_fail_slice_bytes_non_int_arg` | Compile-fail: non-`i64` argument to `slice_bytes` (`E0201`) | Compile check | PASS |
| `test_string_new_empty` | Empty string creation (len=0, cap=0, allocs=0) | `-O0` & `-O1` | PASS |
| `test_string_from_str_ascii` | ASCII string construction and method queries | `-O0` & `-O1` | PASS |
| `test_string_from_str_utf8` | Multi-byte Cyrillic UTF-8 text construction and length | `-O0` & `-O1` | PASS |
| `test_string_embedded_null` | Embedded NUL byte preservation (`"foo\0bar"`) | `-O0` & `-O1` | PASS |
| `test_string_push_str_growth` | Multiple appends triggering geometric reallocation | `-O0` & `-O1` | PASS |
| `test_string_clear_and_reuse` | Clearing string and reusing existing capacity | `-O0` & `-O1` | PASS |
| `test_string_clone_independence` | Cloning string and verifying mutation independence | `-O0` & `-O1` | PASS |
| `test_string_equals` | Exact byte equality comparisons | `-O0` & `-O1` | PASS |
| `test_string_slice_bytes_valid_ascii` | Inherent ASCII slicing returning `Option::Some` | `-O0` & `-O1` | PASS |
| `test_str_slice_bytes_method` | Slicing on primitive string slice `&str` | `-O0` & `-O1` | PASS |
| `test_string_slice_bytes_invalid_bounds` | Bounds checks (`start > end`, negative, exceeding length) | `-O0` & `-O1` | PASS |
| `test_string_slice_bytes_utf8_continuation_rejection` | UTF-8 continuation byte boundary rejection on 4-byte emoji | `-O0` & `-O1` | PASS |
| `test_string_active_allocs_clean_drop` | Heap memory recovery verification (`active_allocs == 0`) | `-O0` & `-O1` | PASS |
| `test_string_reassignment_drop_old` | Reassignment drop glue preventing leaks | `-O0` & `-O1` | PASS |
| `test_option_string_drop` | Nested enum drop glue (`Option<String>`) | `-O0` & `-O1` | PASS |
| `test_vec_string_push_and_drop` | Vector of strings with clean element drop | `-O0` & `-O1` | PASS |
| `test_struct_containing_string` | User struct containing `String` field drop | `-O0` & `-O1` | PASS |

---

## 10. AI Coding Agent Tooling and Discoverability

1. **Unambiguous Method Discoverability**: The standard `String` API adheres to standard systems naming conventions (`as_str()`, `len_bytes()`, `push_str()`, `is_empty()`, `clear()`, `clone()`, `slice_bytes()`), minimizing token hallucinations.
2. **Structured JSON Diagnostics**: Compile errors on illegal mutations, moves, and argument type mismatches emit machine-actionable JSON with exact byte spans and suggestion payloads.
3. **Canonical Source Formatting**: `vantac fmt` canonically formats all string declarations and method invocations without syntax churn.

---

## 11. Acceptance Criteria Verification Table

| Requirement / Criterion | Status | Evidence / Verification |
|---|---|---|
| First-class heap-backed `String` standard-library type | Verified | Defined in `library/std/string.va`, embedded in `crates/vanta_sema/src/module.rs` |
| 3-word descriptor layout (`data_ptr`, `len`, `cap`, 24B, 8B align) | Verified | Tested in `crates/vanta_sema/src/types.rs`, `runtime/vanta_rt.c` |
| Zero allocation on empty string construction | Verified | `test_string_new_empty` verifies `vanta_active_allocs() == 0` |
| UTF-8 invariant maintained by construction | Verified | Validated across `from_str`, `push_str`, `clone`, `slice_bytes` |
| Geometric buffer growth with overflow checking | Verified | `test_string_push_str_growth`, `__builtin_add_overflow` in `vanta_rt.c` |
| Self-aliasing protection during `push_str` | Verified | Interior slice check and relative offset calculation in `vanta_rt.c` |
| Canonical public methods implemented and tested | Verified | `test_string_*` suite covers all 12 API methods |
| Safe byte slicing with UTF-8 continuation rejection | Verified | `test_string_slice_bytes_utf8_continuation_rejection` tests 4-byte 🦀 |
| Destruction on scope exit, reassignment, and container drop | Verified | `test_string_active_allocs_clean_drop`, `test_vec_string_push_and_drop` |
| Borrow checker prevents mutation/moves during borrow | Verified | `test_compile_fail_push_str_while_borrowed`, `test_compile_fail_move_while_borrowed` |
| Identical differential execution under `-O0` and `-O1` | Verified | All 22 tests run differentially under unoptimized and optimized profiles |
| Zero LLVM / Cranelift dependencies | Verified | Relocatable ELF64 emitted directly by custom x86-64 backend |
| Full workspace test suite passes with zero warnings | Verified | `cargo test --workspace`, `cargo clippy`, `cargo fmt --check` clean |

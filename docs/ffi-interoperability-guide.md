# Foreign Function Interface (FFI) & Multi-Language Interoperability in Venta

Venta provides first-class, zero-overhead interoperability with external languages via the standard C ABI (System V AMD64 on Linux/macOS and Microsoft x64 on Windows).

Any native code compiled from **C, C++, Rust, Zig, Go, Fortran, or Assembly** that exports C-ABI symbols can be declared and called directly from Venta without wrappers or glue code.

---

## 1. Syntax

### Block Syntax

Declare multiple foreign functions inside an `extern "C"` block:

```venta
extern "C" {
    pub fn abs(x: i64) -> i64;
    pub fn labs(x: i64) -> i64;
    pub fn puts(s: str) -> i32;
}
```

### Standalone Function Syntax

Declare individual foreign functions with `extern "C" fn`:

```venta
pub extern "C" fn sqrt(x: f64) -> f64;
extern "C" fn getpid() -> i32;
```

---

## 2. Supported Types

| Venta Type | C Equivalent | Rust Equivalent |
|---|---|---|
| `i64` | `long long` / `int64_t` | `i64` |
| `i32` | `int` / `int32_t` | `i32` |
| `i16` | `short` / `int16_t` | `i16` |
| `i8` | `signed char` / `int8_t` | `i8` |
| `u64` | `unsigned long long` / `uint64_t` | `u64` |
| `u32` | `unsigned int` / `uint32_t` | `u32` |
| `u16` | `unsigned short` / `uint16_t` | `u16` |
| `u8` | `unsigned char` / `uint8_t` | `u8` |
| `f64` | `double` | `f64` |
| `f32` | `float` | `f32` |
| `bool` | `bool` / `int8_t` | `bool` |
| `&str` | Slice `(const char*, size_t)` | `&str` |

---

## 3. Invoking C Code

### Step 1: Write C code (`math_utils.c`)

```c
long long c_multiply_accumulate(long long a, long long b, long long c) {
    return (a * b) + c;
}
```

### Step 2: Compile to an object file

```bash
cc -c math_utils.c -o math_utils.o
```

### Step 3: Call from Venta (`app.va`)

```venta
extern "C" {
    pub fn c_multiply_accumulate(a: i64, b: i64, c: i64) -> i64;
}

pub fn main() {
    let result = c_multiply_accumulate(6, 7, 10);
    print_i64(result); // Prints 52
}
```

### Step 4: Run or compile

```bash
# Direct run
vanta run app.va math_utils.o

# Native binary compilation
vanta compile app.va math_utils.o -o my_app
./my_app
```

---

## 4. Invoking Rust Code

### Step 1: Write Rust code (`rust_engine.rs`)

```rust
#![crate_type = "lib"]
#![no_std]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn rust_compute_power(base: i64, exp: i64) -> i64 {
    let mut acc: i64 = 1;
    let mut i: i64 = 0;
    while i < exp {
        acc = acc.wrapping_mul(base);
        i = i.wrapping_add(1);
    }
    acc
}
```

### Step 2: Compile to an object file

```bash
rustc -O -C panic=abort --emit=obj rust_engine.rs -o rust_engine.o
```

### Step 3: Call from Venta

```venta
extern "C" fn rust_compute_power(base: i64, exp: i64) -> i64;

pub fn main() {
    let power = rust_compute_power(2, 10);
    print_i64(power); // Prints 1024
}
```

### Step 4: Run with Venta

```bash
vanta run main.va rust_engine.o
```

---

## 5. Linking System & Third-Party Libraries

Pass library flags directly to `vanta run` and `vanta compile`:

- `-l<name>`: Links library (e.g. `-lsqlite3`, `-lcurl`, `-lm`).
- `-L<dir>`: Adds library search directory.

```bash
# Link SQLite 3 library
vanta run database_app.va -lsqlite3

# Link with custom library path
vanta compile game.va -L/opt/raylib/lib -lraylib -o game_bin
```

# Vanta Canonical Formatting Style

This document defines the authoritative, deterministic canonical source formatting style for the Vanta programming language enforced by `vantac fmt`.

Vanta adopts an explicit AI-first design philosophy: source code must have a single predictable canonical form that is easy for AI coding agents to generate, format, inspect, diff, and repair while remaining clean and ergonomic for human developers.

---

## 1. Indentation and Whitespace

1. **Indentation Unit**: Exactly four (4) ASCII spaces (`0x20`) per indentation level.
2. **No Tabs**: Tab characters (`\t`) are strictly forbidden for indentation.
3. **Trailing Whitespace**: Lines must not contain trailing spaces or tabs.
4. **Blank Lines**: Blank lines must be empty (0 characters before `\n`).
5. **Consecutive Blank Lines**: At most one consecutive blank line is permitted. Redundant multiple blank lines are collapsed into a single blank line.
6. **Trailing Newline**: Every non-empty Vanta source file must terminate with exactly one newline (`\n`).
7. **Line Endings**: Standard Unix line feed (`\n`, LF).

---

## 2. Braces and Blocks

Vanta uses the standard Egyptian / One True Brace Style (1TBS):

1. **Opening Braces**: Opening curly braces (`{`) appear on the same line as the declaration or control flow statement, preceded by a single space.
2. **Closing Braces**: Closing curly braces (`}`) appear on their own line, indented to match the indentation of the construct that introduced the block.
3. **If-Else Chaining**: In conditional branching, `else` and `else if` remain on the same line as the preceding closing brace:
   ```vanta
   if condition {
       // then branch
   } else if other_condition {
       // else-if branch
   } else {
       // else branch
   }
   ```
4. **Empty Blocks**: An empty function or block may be formatted as `{\n}` or on a single line `{}` when compact.

---

## 3. Top-Level Declarations

1. **Spacing Between Items**: Exactly one blank line separates consecutive top-level declarations (`fn`, `struct`, `impl`).
2. **Impl Block Methods**: Exactly one blank line separates methods inside an `impl` block.
3. **Function Signatures**:
   ```vanta
   pub fn calculate<T>(data: &[T], multiplier: i64) -> i64 {
       // body
   }
   ```
4. **Struct Declarations**:
   Each field is placed on its own line indented by one level (4 spaces), with a trailing comma:
   ```vanta
   pub struct Point<T> {
       pub x: T,
       pub y: T,
   }
   ```
5. **Impl Blocks**:
   Methods inside `impl` blocks are indented by one level (4 spaces):
   ```vanta
   impl Point<i64> {
       pub fn translate(self: &mut Self, dx: i64, dy: i64) {
           self.x += dx;
           self.y += dy;
       }
   }
   ```
6. **Trait Declarations**:
   Method signatures inside `trait` blocks are indented by one level (4 spaces), ending with semicolons:
   ```vanta
   pub trait Describe {
       fn describe(self: &Self) -> i64;
   }
   ```
7. **Trait Implementations**:
   `impl Trait for Target` with a single space before and after `for`:
   ```vanta
   impl Describe for Point {
       fn describe(self: &Point) -> i64 {
           self.x + self.y
       }
   }
   ```
8. **Trait Bounds in Type Parameters**:
   No space before `:`, one space after `:`; no padding inside `<...>`:
   ```vanta
   fn render<T: Describe>(item: &T) {
       let val = item.describe();
       print_i64(val);
   }
   ```

---

## 4. Expressions and Operator Spacing

### 4.1 Binary Operators
All binary operators are surrounded by exactly one space on each side:
- **Arithmetic**: `a + b`, `a - b`, `a * b`, `a / b`, `a % b`
- **Comparison**: `a == b`, `a != b`, `a < b`, `a <= b`, `a > b`, `a >= b`
- **Logical**: `a && b`, `a || b`
- **Bitwise**: `a & b`, `a | b`, `a ^ b`, `a << b`, `a >> b`
- **Assignment**: `x = 1`, `x += 1`, `x -= 1`, `x *= 2`, `x /= 2`, `x %= 2`

### 4.2 Unary Operators
Unary operators have no space between the operator symbol and their operand:
- **Negation**: `-x`
- **Logical Not**: `!flag`
- **Dereference**: `*ptr`
- **Immutable Borrow**: `&val`
- **Mutable Borrow**: `&mut val` (single space between `&` and `mut`, no space after `mut`)

### 4.3 Punctuation and Delimiters
- **Semicolons (`;`)**: No space before `;`. Followed by a newline when ending a statement.
- **Commas (`,`)**: No space before `,`. Followed by a single space or newline.
- **Colons (`:`)**: No space before `:`, followed by a single space (`x: i64`, `field: String`).
- **Path Separators (`::`)**: No space before or after `::` (`Module::item`).
- **Member Access (`.`)**: No space before or after `.` (`point.x`, `vec.len()`).
- **Function Calls**: No space between callee and `(`: `sum(a, b)`.
- **Indexing**: No space between array and `[`: `arr[i]`.
- **Parentheses `()` and Brackets `[]`**: No whitespace immediately following `(` or `[`, or immediately preceding `)` or `]`: `(a + b)`, `[1, 2, 3]`.
- **Return Type Arrow (`->`)**: Exactly one space before and after: `fn main() -> i64 {`.
- **Generic Type Arguments / Parameters**: No whitespace inside angle brackets `<...>`: `Vec<i64>`, `identity<T>(val)`. Multiple generic parameters are separated by a comma followed by a single space: `Result<T, E>`.

### 4.4 Postfix `?` Error Propagation Operator
The `?` operator is attached directly to the preceding primary or postfix expression with no intervening whitespace:
- Correct: `parse_port(raw)?`, `step1()?`, `val?`
- Incorrect: `parse_port(raw) ?`
- Chained in binary expressions: `let total = get_a()? + get_b()?;`

### 4.5 Loop Statements and `for ... in ...`
Loop constructs follow standard block spacing rules:
- `while condition { ... }`
- `loop { ... }`
- `for var in iterable { ... }`
  - Exactly one space between `for` and variable identifier: `for x`
  - Exactly one space before and after the `in` keyword: `x in range_i64(0, 5)`
  - Exactly one space between the iterable expression and opening brace: `range_i64(0, 5) {`
  - Body indented by 4 spaces per nesting level.
  - Generic trait implementations with arguments retain no spaces inside angle brackets:
    `impl Iterator<i64> for RangeI64 { ... }`

---

## 5. Comment Preservation Rules

The formatter strictly preserves every comment in its exact semantic position:

1. **Trailing Comments**:
   Comments placed on the same line after code are separated from preceding code by a single space:
   ```vanta
   let timeout = 5000; // timeout in milliseconds
   ```
2. **Standalone Comments**:
   Comments on their own lines are indented to match the indentation level of the surrounding statements:
   ```vanta
   // Initialize lookup table
   let mut table: [i64; 4] = [0, 1, 2, 3];
   ```
3. **Inline Block Comments**:
   Block comments within expressions or argument lists retain a single space before and after:
   ```vanta
   fn compute(a: i64, /* scaling factor */ b: i64) -> i64 {
       a * b
   }
   ```
4. **File Header Comments**:
   Comments at the very top of a source file are preserved at column 1 without alteration.
5. **Nested Block Comments**:
   Vanta supports nested block comments (`/* outer /* inner */ */`), which are preserved verbatim.

---

## 6. Literals and Escapes

The formatter never alters literal content or values:

1. **String Literals**:
   - Double quotes and all escape sequences (`\n`, `\r`, `\t`, `\"`, `\\`, `\0`) are preserved verbatim.
   - Raw UTF-8 string content (such as emojis or non-ASCII identifiers/strings) is preserved byte-for-byte.
2. **Numeric Literals**:
   - Hexadecimal (`0x...`), binary (`0b...`), and octal literals retain their original prefix, casing, and digits.
   - Visual digit grouping underscores (e.g., `1_000_000`) are preserved.
   - Integer type suffixes (e.g., `42i64`, `10u32`) are preserved.

---

## 7. Determinism and Idempotence

Canonical Vanta formatting is strictly idempotent:
```
format(format(source)) == format(source)
```
Repeated invocations of `vantac fmt` on valid Vanta source will produce byte-for-byte identical output without introducing cumulative whitespace, modifying AST structure, or altering program execution semantics.

---

## 8. Modules and Imports Formatting

### 8.1 Module Declarations
- `mod` declarations are formatted with a single space following `pub` (if present) and `mod`:
  ```vanta
  mod math;
  pub mod models;
  ```
- Each module declaration occupies its own line.
- Grouped `mod` declarations are followed by a blank line before subsequent declarations or imports.

### 8.2 Import Statements
- Module paths use `::` with no surrounding whitespace.
- Braced import lists place items inline when concise, separated by a comma and a single space:
  ```vanta
  import math::{add, multiply};
  import models::Point;
  pub import internal::secret;
  ```
- No space before the semicolon.
- Grouped imports are followed by a blank line before the first top-level item.

---

## 9. Lifetimes and Iteration Formatting

### 9.1 Named Lifetime Parameters
- In generic parameter lists, lifetime parameters precede type parameters and are separated by a comma and a space:
  ```vanta
  fn pick<'a, T>(first: &'a T, second: &'a T) -> &'a T { ... }
  struct View<'a, T> { item: &'a T }
  ```
- No space between the apostrophe and the lifetime identifier: `'a`, `'b`, `'static`.

### 9.2 Reference Types with Lifetimes
- In reference types, the lifetime immediately follows the ampersand `&` with no space:
  - Immutable reference: `&'a T` (space between `'a` and `T`)
  - Mutable reference: `&'a mut T` (space between `'a` and `mut`, space between `mut` and `T`)
  - Slices: `&'a [T]`
- In generic type applications containing references or lifetimes:
  - `Option<&'a T>`
  - `Iter<'a, T>`

### 9.3 Collection Iteration Loops
- `for` loops using method iterators are formatted with standard spacing:
  ```vanta
  for item in vector.iter() {
      print_i64(*item);
  }
  ```



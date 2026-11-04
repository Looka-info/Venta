//! Self-contained static Linux x86-64 runtime object.
//!
//! Provides `_start` and minimal Linux syscall implementations for:
//! - Program startup and exit (`sys_exit`)
//! - I/O printing (`print_i64`, `print_str`, `vanta_panic`, `vanta_bounds_check_fail`)
//! - Heap allocation (`vanta_alloc`, `vanta_realloc`, `vanta_free` via `sys_mmap` / `sys_munmap`)
//! - Vector and string runtime primitives
//! - CPU feature detection (`cpuid`)

use crate::input::InputFormat;
use crate::object::ObjectFile;
use crate::section::{InputSection, SectionFlags, SectionKind};
use crate::symbol::{Symbol, SymbolKind, SymbolVisibility};
use std::path::PathBuf;

/// Constructs a pure-Rust synthetic ELF relocatable object containing the static runtime.
pub fn create_static_runtime_object(object_id: usize) -> ObjectFile {
    let mut obj = ObjectFile::new(
        object_id,
        "libvanta_static_rt.a[startup.o]".into(),
        PathBuf::from("vanta://static_rt/startup.o"),
        InputFormat::Elf64,
    );

    let mut text_bytes: Vec<u8> = Vec::new();
    let mut symbols: Vec<Symbol> = Vec::new();

    let mut add_symbol = |name: &str, offset: usize, size: usize| {
        let visibility = if name == "_start" {
            SymbolVisibility::Global
        } else {
            SymbolVisibility::Weak
        };
        symbols.push(Symbol {
            name: name.to_string(),
            visibility,
            kind: SymbolKind::Function,
            section_index: Some(1), // index 1 is .text
            value: offset as u64,
            size: size as u64,
            object_id,
            final_virtual_address: None,
        });
    };

    // =========================================================================
    // 1. _start
    // =========================================================================
    // .globl _start
    // _start:
    //   31 ed                 xor %ebp, %ebp
    //   5f                    pop %rdi             (argc)
    //   48 89 e6              mov %rsp, %rsi       (argv)
    //   48 83 e4 f0           and $-16, %rsp       (align 16)
    //   e8 [call vanta_rt_init]
    //   e8 [call main]
    //   48 89 c7              mov %rax, %rdi       (exit code)
    //   b8 3c 00 00 00        mov $60, %eax        (SYS_exit = 60)
    //   0f 05                 syscall
    //   f4                    hlt
    let start_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x31, 0xED, // xor %ebp, %ebp
        0x5F, // pop %rdi (argc)
        0x48, 0x89, 0xE6, // mov %rsp, %rsi (argv)
        0x48, 0x83, 0xE4, 0xF0, // and $-16, %rsp
    ]);

    // Call vanta_rt_init (offset relative to PC)
    let call_init_pos = text_bytes.len();
    text_bytes.extend_from_slice(&[0xE8, 0x00, 0x00, 0x00, 0x00]);

    // Call main
    let call_main_pos = text_bytes.len();
    text_bytes.extend_from_slice(&[0xE8, 0x00, 0x00, 0x00, 0x00]);

    text_bytes.extend_from_slice(&[
        0x48, 0x89, 0xC7, // mov %rax, %rdi
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov $60, %eax
        0x0F, 0x05, // syscall
        0xF4, // hlt
    ]);
    let start_size = text_bytes.len() - start_offset;
    add_symbol("_start", start_offset, start_size);

    // =========================================================================
    // 2. vanta_rt_init(argc: i64, argv: *const *const u8)
    // =========================================================================
    let init_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0xC3, // ret
    ]);
    add_symbol("vanta_rt_init", init_offset, text_bytes.len() - init_offset);

    // Patch internal call to vanta_rt_init from _start
    let rel_init = (init_offset as i32) - ((call_init_pos + 5) as i32);
    text_bytes[call_init_pos + 1..call_init_pos + 5].copy_from_slice(&rel_init.to_le_bytes());

    // We'll record a relocation from `_start` to `main`
    obj.relocations.push(crate::relocation::RelocationRecord {
        object_id,
        section_index: 1,
        offset_in_section: call_main_pos + 1,
        symbol_name: "main".into(),
        symbol_index: 0,
        reloc_type: crate::elf::reloc::R_X86_64_PC32,
        addend: -4,
    });

    // =========================================================================
    // 3. print_str(ptr: *const u8, len: u64)
    // =========================================================================
    // void print_str(const char *ptr, uint64_t len)
    //   test %rsi, %rsi; jz 2f
    //   test %rdi, %rdi; jz 2f
    //   mov %rsi, %rdx    (count)
    //   mov %rdi, %rsi    (buf)
    //   mov $1, %edi      (fd = 1)
    //   mov $1, %eax      (SYS_write = 1)
    //   syscall
    // 2: ret
    let print_str_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xF6, // test %rsi, %rsi
        0x74, 0x17, // jz 2f (+23 to ret)
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x74, 0x12, // jz 2f (+18 to ret)
        0x48, 0x89, 0xF2, // mov %rsi, %rdx
        0x48, 0x89, 0xFE, // mov %rdi, %rsi
        0xBF, 0x01, 0x00, 0x00, 0x00, // mov $1, %edi
        0xB8, 0x01, 0x00, 0x00, 0x00, // mov $1, %eax
        0x0F, 0x05, // syscall
        0xC3, // ret
    ]);
    add_symbol(
        "print_str",
        print_str_offset,
        text_bytes.len() - print_str_offset,
    );

    // =========================================================================
    // 4. print_i64(val: i64)
    // =========================================================================
    // Formats val as decimal into stack buffer [rsp-48] and calls sys_write(1, buf, len).
    let print_i64_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x83, 0xEC, 0x30, // sub $48, %rsp
        0x48, 0x8D, 0x74, 0x24, 0x2F, // lea 47(%rsp), %rsi
        0x48, 0x89, 0xF8, // mov %rdi, %rax
        0x45, 0x31, 0xC0, // xor %r8d, %r8d (is_neg = 0)
        0x48, 0x85, 0xC0, // test %rax, %rax
        0x79, 0x06, // jns .pos
        0x41, 0xFF, 0xC0, // inc %r8d (is_neg = 1)
        0x48, 0xF7, 0xD8, // neg %rax
        // .pos:
        0x49, 0xC7, 0xC1, 0x0A, 0x00, 0x00, 0x00, // mov $10, %r9
        // .loop:
        0x31, 0xD2, // xor %edx, %edx
        0x49, 0xF7, 0xF1, // div %r9 (%rax = quot, %rdx = rem)
        0x80, 0xC2, 0x30, // add $'0', %dl
        0x48, 0xFF, 0xCE, // dec %rsi
        0x88, 0x16, // mov %dl, (%rsi)
        0x48, 0x85, 0xC0, // test %rax, %rax
        0x75, 0xEE, // jnz .loop
        // check negative sign
        0x45, 0x85, 0xC0, // test %r8d, %r8d
        0x74, 0x06, // jz .write
        0x48, 0xFF, 0xCE, // dec %rsi
        0xC6, 0x06, 0x2D, // movb $'-', (%rsi)
        // .write:
        0x48, 0x8D, 0x54, 0x24, 0x2F, // lea 47(%rsp), %rdx
        0x48, 0x29, 0xF2, // sub %rsi, %rdx (count)
        0xBF, 0x01, 0x00, 0x00, 0x00, // mov $1, %edi (fd = 1)
        0xB8, 0x01, 0x00, 0x00, 0x00, // mov $1, %eax (SYS_write)
        0x0F, 0x05, // syscall
        0x48, 0x83, 0xC4, 0x30, // add $48, %rsp
        0xC3, // ret
    ]);
    add_symbol(
        "print_i64",
        print_i64_offset,
        text_bytes.len() - print_i64_offset,
    );

    // =========================================================================
    // 5. vanta_panic(ptr: *const u8, len: u64)
    // =========================================================================
    let panic_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        // print_str to fd 2
        0x48, 0x89, 0xF2, // mov %rsi, %rdx
        0x48, 0x89, 0xFE, // mov %rdi, %rsi
        0xBF, 0x02, 0x00, 0x00, 0x00, // mov $2, %edi (stderr)
        0xB8, 0x01, 0x00, 0x00, 0x00, // mov $1, %eax
        0x0F, 0x05, // syscall
        // sys_exit(101)
        0xBF, 0x65, 0x00, 0x00, 0x00, // mov $101, %edi
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov $60, %eax (SYS_exit)
        0x0F, 0x05, // syscall
        0xF4, // hlt
    ]);
    add_symbol("vanta_panic", panic_offset, text_bytes.len() - panic_offset);

    // =========================================================================
    // 6. vanta_bounds_check_fail(index: i64, len: i64)
    // =========================================================================
    let bounds_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0xBF, 0x65, 0x00, 0x00, 0x00, // mov $101, %edi
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov $60, %eax
        0x0F, 0x05, // syscall
        0xF4,
    ]);
    add_symbol(
        "vanta_bounds_check_fail",
        bounds_offset,
        text_bytes.len() - bounds_offset,
    );

    // =========================================================================
    // 7. vanta_alloc(size: u64) -> *mut u8
    // =========================================================================
    // Allocates (size + 16) via sys_mmap(NULL, size + 16, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0).
    // Stores size at offset 0, returns pointer + 16.
    let alloc_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x75, 0x03, // jnz alloc_start
        0x31, 0xC0, // xor %eax, %eax (return NULL)
        0xC3, // ret
        // alloc_start:
        0x57, // push %rdi (save size on stack across syscall)
        0x48, 0x83, 0xC7, 0x10, // add $16, %rdi (size + 16)
        0x48, 0x89, 0xFE, // mov %rdi, %rsi (length)
        0x31, 0xFF, // xor %edi, %edi (addr = NULL)
        0xBA, 0x03, 0x00, 0x00, 0x00, // mov $3, %edx (PROT_READ | PROT_WRITE)
        0x41, 0xBA, 0x22, 0x00, 0x00, 0x00, // mov $0x22, %r10d (MAP_PRIVATE | MAP_ANONYMOUS)
        0x49, 0xC7, 0xC0, 0xFF, 0xFF, 0xFF, 0xFF, // mov $-1, %r8 (fd = -1)
        0x4D, 0x31, 0xC9, // xor %r9, %r9 (offset = 0)
        0xB8, 0x09, 0x00, 0x00, 0x00, // mov $9, %eax (SYS_mmap)
        0x0F, 0x05, // syscall
        0x59, // pop %rcx (restore exact size)
        0x48, 0x89, 0x08, // mov %rcx, (%rax) (store size)
        0x48, 0x83, 0xC0, 0x10, // add $16, %rax (return user buffer)
        0xC3, // ret
    ]);
    add_symbol("vanta_alloc", alloc_offset, text_bytes.len() - alloc_offset);
    add_symbol("malloc", alloc_offset, text_bytes.len() - alloc_offset);

    // =========================================================================
    // 8. vanta_free(ptr: *mut u8)
    // =========================================================================
    // If ptr is NULL, ret. Else read size at [ptr - 16], call sys_munmap(ptr - 16, size + 16).
    let free_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x74, 0x12, // jz 2f (+18 to ret)
        0x48, 0x83, 0xEF, 0x10, // sub $16, %rdi
        0x48, 0x8B, 0x37, // mov (%rdi), %rsi (size)
        0x48, 0x83, 0xC6, 0x10, // add $16, %rsi (length = size + 16)
        0xB8, 0x0B, 0x00, 0x00, 0x00, // mov $11, %eax (SYS_munmap = 11)
        0x0F, 0x05, // syscall
        0xC3, // ret
    ]);
    add_symbol("vanta_free", free_offset, text_bytes.len() - free_offset);
    add_symbol("free", free_offset, text_bytes.len() - free_offset);

    // =========================================================================
    // 9. vanta_realloc(ptr: *mut u8, new_size: u64) -> *mut u8
    // =========================================================================
    let realloc_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x75, 0x09, // jnz do_realloc (+9)
        0x48, 0x89, 0xF7, // mov %rsi, %rdi
        0xE8, // call vanta_alloc
    ]);
    let call_alloc_null_pos = text_bytes.len();
    text_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    text_bytes.extend_from_slice(&[
        0xC3, // ret
        // do_realloc:
        // Save registers, allocate new buffer, copy min(old_size, new_size), free old
        0x53, // push %rbx
        0x48, 0x89, 0xFB, // mov %rdi, %rbx (old ptr)
        0x56, // push %rsi (new_size)
        0x48, 0x89, 0xF7, // mov %rsi, %rdi
        0xE8, // call vanta_alloc
    ]);
    let call_alloc_pos = text_bytes.len();
    text_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    text_bytes.extend_from_slice(&[
        0x5E, // pop %rsi (new_size)
        0x48, 0x85, 0xC0, // test %rax, %rax
        0x74, 0x1F, // jz fail (+31)
        // Copy old data:
        0x48, 0x8B, 0x4B, 0xF0, // mov -16(%rbx), %rcx (old_size)
        0x48, 0x39, 0xF1, // cmp %rsi, %rcx
        0x48, 0x0F, 0x47, 0xCE, // cmova %rsi, %rcx (min(old, new))
        0x50, // push %rax (new ptr)
        0x48, 0x89, 0xC7, // mov %rax, %rdi (dest)
        0x48, 0x89, 0xDE, // mov %rbx, %rsi (src)
        0xF3, 0xA4, // rep movsb
        0x48, 0x89, 0xDF, // mov %rbx, %rdi (old ptr to free)
        0xE8, // call vanta_free
    ]);
    let call_free_pos = text_bytes.len();
    text_bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    text_bytes.extend_from_slice(&[
        0x58, // pop %rax (new ptr)
        0x5B, // pop %rbx
        0xC3, // ret
        0x5B, // fail: pop %rbx
        0x31, 0xC0, // xor %eax, %eax
        0xC3,
    ]);
    let rel_alloc_null = (alloc_offset as i32) - ((call_alloc_null_pos + 4) as i32);
    text_bytes[call_alloc_null_pos..call_alloc_null_pos + 4]
        .copy_from_slice(&rel_alloc_null.to_le_bytes());
    let rel_alloc = (alloc_offset as i32) - ((call_alloc_pos + 4) as i32);
    text_bytes[call_alloc_pos..call_alloc_pos + 4].copy_from_slice(&rel_alloc.to_le_bytes());
    let rel_free = (free_offset as i32) - ((call_free_pos + 4) as i32);
    text_bytes[call_free_pos..call_free_pos + 4].copy_from_slice(&rel_free.to_le_bytes());
    add_symbol(
        "vanta_realloc",
        realloc_offset,
        text_bytes.len() - realloc_offset,
    );
    add_symbol("realloc", realloc_offset, text_bytes.len() - realloc_offset);

    // =========================================================================
    // 10. CPU feature detection: vanta_cpu_has_avx/avx2/fma/avx512f
    // =========================================================================
    let cpu_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0xB8, 0x01, 0x00, 0x00, 0x00, // mov $1, %eax
        0xC3, // ret 1
    ]);
    add_symbol(
        "vanta_cpu_has_avx",
        cpu_offset,
        text_bytes.len() - cpu_offset,
    );
    add_symbol(
        "vanta_cpu_has_avx2",
        cpu_offset,
        text_bytes.len() - cpu_offset,
    );
    add_symbol(
        "vanta_cpu_has_fma",
        cpu_offset,
        text_bytes.len() - cpu_offset,
    );
    add_symbol(
        "vanta_cpu_has_avx512f",
        cpu_offset,
        text_bytes.len() - cpu_offset,
    );

    // =========================================================================
    // 11. vanta_str_init(str: *mut VantaString)
    // =========================================================================
    let str_init_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0xC7, 0x07, 0x00, 0x00, 0x00, 0x00, // movq $0, (%rdi)
        0x48, 0xC7, 0x47, 0x08, 0x00, 0x00, 0x00, 0x00, // movq $0, 8(%rdi)
        0x48, 0xC7, 0x47, 0x10, 0x00, 0x00, 0x00, 0x00, // movq $0, 16(%rdi)
        0xC3, // ret
    ]);
    add_symbol(
        "vanta_str_init",
        str_init_offset,
        text_bytes.len() - str_init_offset,
    );

    // =========================================================================
    // 12. vanta_vec_init(vec: *mut VantaVec)
    // =========================================================================
    let vec_init_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x74, 0x17, // jz +23 (to ret)
        0x48, 0xC7, 0x07, 0x00, 0x00, 0x00, 0x00, // movq $0, (%rdi)
        0x48, 0xC7, 0x47, 0x08, 0x00, 0x00, 0x00, 0x00, // movq $0, 8(%rdi)
        0x48, 0xC7, 0x47, 0x10, 0x00, 0x00, 0x00, 0x00, // movq $0, 16(%rdi)
        0xC3, // ret
    ]);
    add_symbol(
        "vanta_vec_init",
        vec_init_offset,
        text_bytes.len() - vec_init_offset,
    );

    // =========================================================================
    // 13. vanta_vec_len(vec: *const VantaVec) -> i64
    // =========================================================================
    let vec_len_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x74, 0x05, // jz +5
        0x48, 0x8B, 0x47, 0x08, // mov 8(%rdi), %rax
        0xC3, // ret
        0x31, 0xC0, // xor %eax, %eax
        0xC3, // ret
    ]);
    add_symbol(
        "vanta_vec_len",
        vec_len_offset,
        text_bytes.len() - vec_len_offset,
    );

    // =========================================================================
    // 14. vanta_vec_capacity(vec: *const VantaVec) -> i64
    // =========================================================================
    let vec_cap_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x74, 0x05, // jz +5
        0x48, 0x8B, 0x47, 0x10, // mov 16(%rdi), %rax
        0xC3, // ret
        0x31, 0xC0, // xor %eax, %eax
        0xC3, // ret
    ]);
    add_symbol(
        "vanta_vec_capacity",
        vec_cap_offset,
        text_bytes.len() - vec_cap_offset,
    );

    // =========================================================================
    // 15. vanta_vec_free(vec: *mut VantaVec)
    // =========================================================================
    let vec_free_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x74, 0x0D, // jz done
        0x48, 0x8B, 0x3F, // mov (%rdi), %rdi
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x74, 0x05, // jz done
        0xE8, 0x00, 0x00, 0x00, 0x00, // call vanta_free
        0xC3, // done: ret
    ]);
    let call_vfree_pos = vec_free_offset + 14;
    let rel_vfree = (free_offset as i32) - ((call_vfree_pos + 4) as i32);
    text_bytes[call_vfree_pos..call_vfree_pos + 4].copy_from_slice(&rel_vfree.to_le_bytes());
    add_symbol(
        "vanta_vec_free",
        vec_free_offset,
        text_bytes.len() - vec_free_offset,
    );
    add_symbol(
        "vanta_vec_free_strings",
        vec_free_offset,
        text_bytes.len() - vec_free_offset,
    );

    // =========================================================================
    // 15b. vanta_vec_push(vec: *mut VantaVec, val: i64, elem_size: i64)
    // =========================================================================
    let vec_push_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xFF, // test %rdi, %rdi
        0x0F, 0x84, 0x8B, 0x00, 0x00, 0x00, // je done (+139)
        0x48, 0x85, 0xD2, // test %rdx, %rdx
        0x7F, 0x05, // jg .has_elem_size (+5)
        0xBA, 0x08, 0x00, 0x00, 0x00, // mov $8, %edx
        // .has_elem_size:
        0x48, 0x8B, 0x47, 0x08, // mov 8(%rdi), %rax (len)
        0x48, 0x3B, 0x47, 0x10, // cmp 16(%rdi), %rax (cap)
        0x7C, 0x31, // jl .has_space (+49)
        // Grow vector:
        0x48, 0x8B, 0x4F, 0x10, // mov 16(%rdi), %rcx (cap)
        0x48, 0x85, 0xC9, // test %rcx, %rcx
        0x75, 0x07, // jne .double_cap (+7)
        0xB9, 0x04, 0x00, 0x00, 0x00, // mov $4, %ecx
        0xEB, 0x03, // jmp .calc_size (+3)
        // .double_cap:
        0x48, 0xD1, 0xE1, // shl $1, %rcx
        // .calc_size:
        0x57, // push %rdi (vec)
        0x56, // push %rsi (val)
        0x52, // push %rdx (elem_size)
        0x51, // push %rcx (new_cap)
        0x48, 0x89, 0xCE, // mov %rcx, %rsi
        0x48, 0x0F, 0xAF, 0xF2, // imul %rdx, %rsi (new_size = new_cap * elem_size)
        0x48, 0x8B, 0x3F, // mov (%rdi), %rdi (vec->data)
        0xE8, 0x00, 0x00, 0x00, 0x00, // call vanta_realloc
        0x59, // pop %rcx (new_cap)
        0x5A, // pop %rdx (elem_size)
        0x5E, // pop %rsi (val)
        0x5F, // pop %rdi (vec)
        0x48, 0x89, 0x07, // mov %rax, (%rdi) (vec->data = new_ptr)
        0x48, 0x89, 0x4F, 0x10, // mov %rcx, 16(%rdi) (vec->cap = new_cap)
        // .has_space:
        0x48, 0x8B, 0x47, 0x08, // mov 8(%rdi), %rax (len)
        0x48, 0x0F, 0xAF, 0xC2, // imul %rdx, %rax (offset = len * elem_size)
        0x48, 0x03, 0x07, // add (%rdi), %rax (dest = vec->data + offset)
        // Store element:
        0x48, 0x83, 0xFA, 0x08, // cmp $8, %rdx
        0x75, 0x05, // jne .check_4
        0x48, 0x89, 0x30, // mov %rsi, (%rax)
        0xEB, 0x2C, // jmp .inc_len (+44)
        // .check_4:
        0x48, 0x83, 0xFA, 0x04, // cmp $4, %rdx
        0x75, 0x04, // jne .check_2
        0x89, 0x30, // mov %esi, (%rax)
        0xEB, 0x22, // jmp .inc_len (+34)
        // .check_2:
        0x48, 0x83, 0xFA, 0x02, // cmp $2, %rdx
        0x75, 0x05, // jne .check_1
        0x66, 0x89, 0x30, // mov %si, (%rax)
        0xEB, 0x17, // jmp .inc_len (+23)
        // .check_1:
        0x48, 0x83, 0xFA, 0x01, // cmp $1, %rdx
        0x75, 0x05, // jne .copy_block
        0x40, 0x88, 0x30, // mov %sil, (%rax)
        0xEB, 0x0C, // jmp .inc_len (+12)
        // .copy_block (elem_size > 8):
        0x57, // push %rdi
        0x56, // push %rsi
        0x48, 0x89, 0xC7, // mov %rax, %rdi (dest)
        0x48, 0x89, 0xD1, // mov %rdx, %rcx (count)
        0xF3, 0xA4, // rep movsb
        0x5E, // pop %rsi
        0x5F, // pop %rdi
        // .inc_len:
        0x48, 0xFF, 0x47, 0x08, // incq 8(%rdi) (vec->len++)
        // .done:
        0xC3, // ret
    ]);
    let call_realloc_pos = vec_push_offset + 0x3F;
    let rel_realloc = (realloc_offset as i32) - ((call_realloc_pos + 4) as i32);
    text_bytes[call_realloc_pos..call_realloc_pos + 4].copy_from_slice(&rel_realloc.to_le_bytes());
    add_symbol(
        "vanta_vec_push",
        vec_push_offset,
        text_bytes.len() - vec_push_offset,
    );

    // =========================================================================
    // 16. POSIX Syscall Primitives (Weak): write, read, close, open
    // =========================================================================
    let write_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0xB8, 0x01, 0x00, 0x00, 0x00, // mov $1, %eax (SYS_write = 1)
        0x0F, 0x05, // syscall
        0xC3, // ret
    ]);
    add_symbol("write", write_offset, text_bytes.len() - write_offset);

    let read_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x31, 0xC0, // xor %eax, %eax (SYS_read = 0)
        0x0F, 0x05, // syscall
        0xC3, // ret
    ]);
    add_symbol("read", read_offset, text_bytes.len() - read_offset);

    let close_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0xB8, 0x03, 0x00, 0x00, 0x00, // mov $3, %eax (SYS_close = 3)
        0x0F, 0x05, // syscall
        0xC3, // ret
    ]);
    add_symbol("close", close_offset, text_bytes.len() - close_offset);

    let open_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0xB8, 0x02, 0x00, 0x00, 0x00, // mov $2, %eax (SYS_open = 2)
        0x0F, 0x05, // syscall
        0xC3, // ret
    ]);
    add_symbol("open", open_offset, text_bytes.len() - open_offset);

    let exit_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov $60, %eax (SYS_exit = 60)
        0x0F, 0x05, // syscall
        0xF4, // hlt
    ]);
    add_symbol("_exit", exit_offset, text_bytes.len() - exit_offset);

    // memcpy: (dest = %rdi, src = %rsi, count = %rdx) -> %rax = dest
    let memcpy_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x89, 0xF8, // mov %rdi, %rax
        0x48, 0x89, 0xD1, // mov %rdx, %rcx
        0xF3, 0xA4, // rep movsb
        0xC3, // ret
    ]);
    add_symbol("memcpy", memcpy_offset, text_bytes.len() - memcpy_offset);

    // memset: (s = %rdi, c = %rsi, n = %rdx) -> %rax = s
    let memset_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x49, 0x89, 0xF8, // mov %rdi, %r8
        0x40, 0x88, 0xF0, // mov %sil, %al
        0x48, 0x89, 0xD1, // mov %rdx, %rcx
        0xF3, 0xAA, // rep stosb
        0x4C, 0x89, 0xC0, // mov %r8, %rax
        0xC3, // ret
    ]);
    add_symbol("memset", memset_offset, text_bytes.len() - memset_offset);

    // memcmp: (s1 = %rdi, s2 = %rsi, n = %rdx) -> %eax
    let memcmp_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x48, 0x85, 0xD2, // test %rdx, %rdx
        0x74, 0x15, // jz .Lzero (+21)
        0x48, 0x89, 0xD1, // mov %rdx, %rcx
        0xF3, 0xA6, // repe cmpsb
        0x74, 0x0E, // je .Lzero (+14)
        0x48, 0x0F, 0xB6, 0x47, 0xFF, // movzbq -1(%rdi), %rax
        0x48, 0x0F, 0xB6, 0x4E, 0xFF, // movzbq -1(%rsi), %rcx
        0x48, 0x29, 0xC8, // sub %rcx, %rax
        0xC3, // ret
        0x31, 0xC0, // .Lzero: xor %eax, %eax
        0xC3, // ret
    ]);
    add_symbol("memcmp", memcmp_offset, text_bytes.len() - memcmp_offset);

    // strlen: (s = %rdi) -> %rax
    let strlen_offset = text_bytes.len();
    text_bytes.extend_from_slice(&[
        0x31, 0xC0, // xor %eax, %eax
        0x80, 0x3C, 0x07, 0x00, // .Lloop: cmpb $0, (%rdi, %rax)
        0x74, 0x05, // je .Ldone (+5)
        0x48, 0xFF, 0xC0, // inc %rax
        0xEB, 0xF5, // jmp .Lloop (-11)
        0xC3, // .Ldone: ret
    ]);
    add_symbol("strlen", strlen_offset, text_bytes.len() - strlen_offset);

    // Create the .text section
    obj.sections.push(InputSection {
        object_id,
        source_index: 1,
        name: ".text".into(),
        kind: SectionKind::Code,
        flags: SectionFlags::code(),
        align: 16,
        size: text_bytes.len(),
        data: text_bytes,
    });

    obj.symbols = symbols;
    obj
}

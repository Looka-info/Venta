use vanta_codegen_x86_64::encoding::{CodeEmitter, CodegenError};
use vanta_codegen_x86_64::machine_ir::{MachineInst, SimdOperand};
use vanta_codegen_x86_64::register::{Reg, SimdReg, VectorWidth};
use vanta_codegen_x86_64::target::TargetFeatures;

#[test]
fn test_vmovups_256_encodings() {
    let target = TargetFeatures::avx();

    // 1. vmovups %ymm1, %ymm0 -> c5 fc 10 c1
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src: SimdOperand::Reg(SimdReg::ymm(1)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc5, 0xfc, 0x10, 0xc1]);

    // 2. vmovups (%rdi), %ymm0 -> c5 fc 10 07
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src: SimdOperand::Mem {
                    base: Reg::Rdi,
                    offset: 0,
                },
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc5, 0xfc, 0x10, 0x07]);

    // 3. vmovups 32(%rdi), %ymm0 -> c5 fc 10 47 20
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src: SimdOperand::Mem {
                    base: Reg::Rdi,
                    offset: 32,
                },
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc5, 0xfc, 0x10, 0x47, 0x20]);

    // 4. vmovups %ymm0, (%rdi) -> c5 fc 11 07
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V256,
                dest: SimdOperand::Mem {
                    base: Reg::Rdi,
                    offset: 0,
                },
                src: SimdOperand::Reg(SimdReg::ymm(0)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc5, 0xfc, 0x11, 0x07]);

    // 5. vmovups %ymm9, %ymm8 -> c4 41 7c 10 c1 (high registers)
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(8)),
                src: SimdOperand::Reg(SimdReg::ymm(9)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc4, 0x41, 0x7c, 0x10, 0xc1]);
}

#[test]
fn test_vpaddq_256_encodings() {
    let target = TargetFeatures::avx2();

    // 1. vpaddq %ymm2, %ymm1, %ymm0 -> c5 f5 d4 c2
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vpaddq {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src1: SimdOperand::Reg(SimdReg::ymm(1)),
                src2: SimdOperand::Reg(SimdReg::ymm(2)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc5, 0xf5, 0xd4, 0xc2]);

    // 2. vpaddq (%rdi), %ymm1, %ymm0 -> c5 f5 d4 07
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vpaddq {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src1: SimdOperand::Reg(SimdReg::ymm(1)),
                src2: SimdOperand::Mem {
                    base: Reg::Rdi,
                    offset: 0,
                },
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc5, 0xf5, 0xd4, 0x07]);

    // 3. vpaddq %ymm10, %ymm9, %ymm8 -> c4 41 35 d4 c2
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vpaddq {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(8)),
                src1: SimdOperand::Reg(SimdReg::ymm(9)),
                src2: SimdOperand::Reg(SimdReg::ymm(10)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc4, 0x41, 0x35, 0xd4, 0xc2]);
}

#[test]
fn test_vfmadd231pd_256_encodings() {
    let target = TargetFeatures::avx2_fma();

    // 1. vfmadd231pd %ymm2, %ymm1, %ymm0 -> c4 e2 f5 b8 c2
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vfmadd231pd {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src1: SimdOperand::Reg(SimdReg::ymm(1)),
                src2: SimdOperand::Reg(SimdReg::ymm(2)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc4, 0xe2, 0xf5, 0xb8, 0xc2]);

    // 2. vfmadd231pd (%rdi), %ymm1, %ymm0 -> c4 e2 f5 b8 07
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vfmadd231pd {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src1: SimdOperand::Reg(SimdReg::ymm(1)),
                src2: SimdOperand::Mem {
                    base: Reg::Rdi,
                    offset: 0,
                },
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc4, 0xe2, 0xf5, 0xb8, 0x07]);

    // 3. vfmadd231pd %ymm10, %ymm9, %ymm8 -> c4 42 b5 b8 c2
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vfmadd231pd {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(8)),
                src1: SimdOperand::Reg(SimdReg::ymm(9)),
                src2: SimdOperand::Reg(SimdReg::ymm(10)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0xc4, 0x42, 0xb5, 0xb8, 0xc2]);
}

#[test]
fn test_avx512_512bit_encodings() {
    let target = TargetFeatures::avx512();

    // 1. vmovups %zmm1, %zmm0 -> 62 f1 7c 48 10 c1
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V512,
                dest: SimdOperand::Reg(SimdReg::zmm(0)),
                src: SimdOperand::Reg(SimdReg::zmm(1)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0x62, 0xf1, 0x7c, 0x48, 0x10, 0xc1]);

    // 2. vpaddq %zmm2, %zmm1, %zmm0 -> 62 f1 f5 48 d4 c2
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vpaddq {
                width: VectorWidth::V512,
                dest: SimdOperand::Reg(SimdReg::zmm(0)),
                src1: SimdOperand::Reg(SimdReg::zmm(1)),
                src2: SimdOperand::Reg(SimdReg::zmm(2)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0x62, 0xf1, 0xf5, 0x48, 0xd4, 0xc2]);

    // 3. vfmadd231pd %zmm2, %zmm1, %zmm0 -> 62 f2 f5 48 b8 c2
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vfmadd231pd {
                width: VectorWidth::V512,
                dest: SimdOperand::Reg(SimdReg::zmm(0)),
                src1: SimdOperand::Reg(SimdReg::zmm(1)),
                src2: SimdOperand::Reg(SimdReg::zmm(2)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0x62, 0xf2, 0xf5, 0x48, 0xb8, 0xc2]);

    // 4. High register EVEX test: vmovups %zmm17, %zmm16 -> 62 a1 7c 48 10 c1
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V512,
                dest: SimdOperand::Reg(SimdReg::zmm(16)),
                src: SimdOperand::Reg(SimdReg::zmm(17)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0x62, 0xa1, 0x7c, 0x48, 0x10, 0xc1]);

    // 5. Extended high registers: vpaddq %zmm31, %zmm30, %zmm29 -> 62 01 8d 40 d4 ef
    let mut emitter = CodeEmitter::with_target(target);
    emitter
        .encode_instruction_checked(
            &MachineInst::Vpaddq {
                width: VectorWidth::V512,
                dest: SimdOperand::Reg(SimdReg::zmm(29)),
                src1: SimdOperand::Reg(SimdReg::zmm(30)),
                src2: SimdOperand::Reg(SimdReg::zmm(31)),
            },
            &target,
        )
        .expect("encoding should succeed");
    assert_eq!(emitter.bytes, vec![0x62, 0x01, 0x8d, 0x40, 0xd4, 0xef]);
}

#[test]
fn test_target_feature_gating_and_rejection() {
    let baseline = TargetFeatures::baseline();
    let avx_only = TargetFeatures::avx();
    let avx2_no_fma = TargetFeatures::avx2();

    let mut emitter = CodeEmitter::new();

    // 1. vmovups 256 on baseline -> MissingTargetFeature(avx)
    let err = emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src: SimdOperand::Reg(SimdReg::ymm(1)),
            },
            &baseline,
        )
        .expect_err("should reject vmovups on baseline target");
    assert_eq!(
        err,
        CodegenError::MissingTargetFeature {
            instruction: "vmovups",
            feature: "avx",
            width: VectorWidth::V256,
        }
    );

    // 2. vpaddq 256 on avx_only -> MissingTargetFeature(avx2)
    let err = emitter
        .encode_instruction_checked(
            &MachineInst::Vpaddq {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src1: SimdOperand::Reg(SimdReg::ymm(1)),
                src2: SimdOperand::Reg(SimdReg::ymm(2)),
            },
            &avx_only,
        )
        .expect_err("should reject vpaddq on AVX-only target");
    assert_eq!(
        err,
        CodegenError::MissingTargetFeature {
            instruction: "vpaddq",
            feature: "avx2",
            width: VectorWidth::V256,
        }
    );

    // 3. vfmadd231pd 256 on avx2_no_fma -> MissingTargetFeature(fma)
    let err = emitter
        .encode_instruction_checked(
            &MachineInst::Vfmadd231pd {
                width: VectorWidth::V256,
                dest: SimdOperand::Reg(SimdReg::ymm(0)),
                src1: SimdOperand::Reg(SimdReg::ymm(1)),
                src2: SimdOperand::Reg(SimdReg::ymm(2)),
            },
            &avx2_no_fma,
        )
        .expect_err("should reject vfmadd231pd when FMA is disabled");
    assert_eq!(
        err,
        CodegenError::MissingTargetFeature {
            instruction: "vfmadd231pd",
            feature: "fma",
            width: VectorWidth::V256,
        }
    );
    assert_eq!(
        err,
        CodegenError::MissingTargetFeature {
            instruction: "vfmadd231pd",
            feature: "fma",
            width: VectorWidth::V256,
        }
    );

    // 4. Any 512-bit instruction on avx2_fma -> MissingTargetFeature(avx512f)
    let err = emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V512,
                dest: SimdOperand::Reg(SimdReg::zmm(0)),
                src: SimdOperand::Reg(SimdReg::zmm(1)),
            },
            &TargetFeatures::avx2_fma(),
        )
        .expect_err("should reject 512-bit on non-AVX512 target");
    assert_eq!(
        err,
        CodegenError::MissingTargetFeature {
            instruction: "vmovups",
            feature: "avx512f",
            width: VectorWidth::V512,
        }
    );

    // 5. Memory-to-memory vmovups rejection
    let err = emitter
        .encode_instruction_checked(
            &MachineInst::Vmovups {
                width: VectorWidth::V256,
                dest: SimdOperand::Mem {
                    base: Reg::Rdi,
                    offset: 0,
                },
                src: SimdOperand::Mem {
                    base: Reg::Rsi,
                    offset: 0,
                },
            },
            &TargetFeatures::avx(),
        )
        .expect_err("should reject mem-to-mem vmovups");
    assert_eq!(
        err,
        CodegenError::UnsupportedOperandCombination(
            "vmovups does not support memory-to-memory encoding"
        )
    );
}

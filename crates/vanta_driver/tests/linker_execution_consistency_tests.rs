//! Test suite for Venta Fix 2.2:
//! Guarantee Effective Linker Execution Matches Its Fingerprint.
//!
//! Validates:
//! 1. Plan resolving backend A must not execute backend B (verification failure blocks execution).
//! 2. Driver dry-run result selecting backend B must not retain fingerprint for backend A.
//! 3. Changing -fuse-ld must change effective backend and fingerprint.
//! 4. Changing -B must update or invalidate resolved backend and fingerprint.
//! 5. Exact resolved backend path must be the backend used by actual invocation (verified via executed_links).
//! 6. Replacing backend between resolution and execution must abort with hash mismatch error.
//! 7. Driver that cannot provide trustworthy effective backend fails closed (returns Err, no cache entry).
//! 8. Identical resolved invocations produce deterministic fingerprints.
//! 9. Internal linking works without resolving external linker binaries.

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use vanta_driver::linker_resolver::{MockExecution, MockLinkerResolver};
use vanta_driver::{CompilerIdentity, CompilerOptions, CompilerSession, LinkerKind, TargetTriple};

fn dummy_compiler_id() -> CompilerIdentity {
    CompilerIdentity {
        compiler_version: "2.1.0".into(),
        cache_schema_version: 1,
        git_revision: "fix2_2".into(),
        target_triple: "x86_64-unknown-linux-gnu".into(),
        source_digest: "1111111111111111111111111111111111111111111111111111111111111111".into(),
    }
}

#[test]
fn test_1_plan_resolving_backend_a_cannot_execute_backend_b() {
    let dir = tempdir().unwrap();
    let target_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();

    let fake_rt = dir.path().join("libvanta_rt.a");
    fs::write(&fake_rt, b"FAKE_RUNTIME_ARCHIVE").unwrap();

    let mut opts = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts.linker = LinkerKind::System;
    opts.runtime_path = Some(fake_rt);

    // Resolver at plan resolution time: selects backend A (/usr/bin/ld.bfd)
    let resolver_a = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"GCC_DRIVER", Some("gcc 12"))
        .with_downstream("gcc", None, Ok(Some(PathBuf::from("/usr/bin/ld.bfd"))))
        .with_executable(
            "ld.bfd",
            "/usr/bin/ld.bfd",
            b"LD_BFD_BACKEND",
            Some("GNU ld (bfd) 2.40"),
        );

    let plan_a = CompilerSession::resolve_link_plan_with_resolver(&opts, &target_spec, &resolver_a)
        .expect("Plan A resolution should succeed");
    assert_eq!(
        plan_a.downstream_linker.as_ref().unwrap().path,
        Path::new("/usr/bin/ld.bfd")
    );

    // Runner at link execution time: backend A still exists on disk with matching content,
    // but the driver's dry-run probe discovers the driver would actually invoke backend B (/usr/bin/ld.gold)
    let runner_b = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"GCC_DRIVER", Some("gcc 12"))
        .with_downstream("gcc", None, Ok(Some(PathBuf::from("/usr/bin/ld.gold"))))
        .with_executable(
            "ld.bfd",
            "/usr/bin/ld.bfd",
            b"LD_BFD_BACKEND",
            Some("GNU ld (bfd) 2.40"),
        )
        .with_executable(
            "ld.gold",
            "/usr/bin/ld.gold",
            b"LD_GOLD_BACKEND",
            Some("GNU gold 1.16"),
        );

    let res = CompilerSession::link_target_executable_with_plan_and_resolver(
        &opts,
        &target_spec,
        b"DUMMY_OBJ",
        &plan_a,
        &runner_b,
    );

    assert!(
        res.is_err(),
        "Executing backend B with plan resolved for backend A must be rejected"
    );
    let err_msg = res.unwrap_err();
    assert!(
        err_msg.contains("Driver verification failed"),
        "Error message must indicate driver verification failure: {}",
        err_msg
    );
    assert!(
        err_msg.contains("ld.gold") && err_msg.contains("ld.bfd"),
        "Error message must show actual vs expected backend mismatch: {}",
        err_msg
    );

    // Crucial check: verify that execution was completely blocked before spawning any process
    let executions = runner_b.get_executed_links();
    assert_eq!(
        executions.len(),
        0,
        "No link execution should be performed when backend verification fails"
    );
}

#[test]
fn test_2_dry_run_selecting_backend_b_does_not_retain_fingerprint_for_backend_a() {
    let dir = tempdir().unwrap();
    let cid = dummy_compiler_id();
    let target_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();

    let fake_rt = dir.path().join("libvanta_rt.a");
    fs::write(&fake_rt, b"FAKE_RUNTIME_ARCHIVE").unwrap();

    let mut opts = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts.linker = LinkerKind::System;
    opts.runtime_path = Some(fake_rt);

    let resolver_a = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"GCC_DRIVER", None)
        .with_downstream("gcc", None, Ok(Some(PathBuf::from("/usr/bin/ld.bfd"))))
        .with_executable("ld.bfd", "/usr/bin/ld.bfd", b"LD_BFD_BACKEND", None);

    let resolver_b = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"GCC_DRIVER", None)
        .with_downstream("gcc", None, Ok(Some(PathBuf::from("/usr/bin/ld.gold"))))
        .with_executable("ld.gold", "/usr/bin/ld.gold", b"LD_GOLD_BACKEND", None);

    let plan_a =
        CompilerSession::resolve_link_plan_with_resolver(&opts, &target_spec, &resolver_a).unwrap();
    let plan_b =
        CompilerSession::resolve_link_plan_with_resolver(&opts, &target_spec, &resolver_b).unwrap();

    let fp_a = plan_a.fingerprint(&cid, &[]).unwrap();
    let fp_b = plan_b.fingerprint(&cid, &[]).unwrap();

    assert_ne!(
        fp_a, fp_b,
        "Driver dry-run selecting backend B must produce different fingerprint from backend A"
    );

    // Furthermore, plan A cannot be executed on environment B
    let res = CompilerSession::link_target_executable_with_plan_and_resolver(
        &opts,
        &target_spec,
        b"DUMMY_OBJ",
        &plan_a,
        &resolver_b,
    );
    assert!(
        res.is_err(),
        "Attempting to execute plan A on backend B must fail closed"
    );
}

#[test]
fn test_3_changing_fuse_ld_changes_effective_backend_and_fingerprint() {
    let dir = tempdir().unwrap();
    let cid = dummy_compiler_id();
    let target_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();

    let fake_rt = dir.path().join("libvanta_rt.a");
    fs::write(&fake_rt, b"FAKE_RUNTIME_ARCHIVE").unwrap();

    let resolver = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"GCC_DRIVER", None)
        .with_downstream(
            "gcc",
            Some("-fuse-ld=bfd"),
            Ok(Some(PathBuf::from("/usr/bin/ld.bfd"))),
        )
        .with_downstream(
            "gcc",
            Some("-fuse-ld=lld"),
            Ok(Some(PathBuf::from("/usr/bin/ld.lld"))),
        )
        .with_executable("ld.bfd", "/usr/bin/ld.bfd", b"LD_BFD_CONTENT", None)
        .with_executable("ld.lld", "/usr/bin/ld.lld", b"LD_LLD_CONTENT", None);

    let mut opts1 = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts1.linker = LinkerKind::System;
    opts1.runtime_path = Some(fake_rt.clone());
    opts1.linker_flags = vec!["-fuse-ld=bfd".into()];

    let mut opts2 = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts2.linker = LinkerKind::System;
    opts2.runtime_path = Some(fake_rt);
    opts2.linker_flags = vec!["-fuse-ld=lld".into()];

    let plan1 =
        CompilerSession::resolve_link_plan_with_resolver(&opts1, &target_spec, &resolver).unwrap();
    let plan2 =
        CompilerSession::resolve_link_plan_with_resolver(&opts2, &target_spec, &resolver).unwrap();

    assert_eq!(
        plan1.downstream_linker.as_ref().unwrap().path,
        Path::new("/usr/bin/ld.bfd")
    );
    assert_eq!(
        plan2.downstream_linker.as_ref().unwrap().path,
        Path::new("/usr/bin/ld.lld")
    );

    let fp1 = plan1.fingerprint(&cid, &[]).unwrap();
    let fp2 = plan2.fingerprint(&cid, &[]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Changing -fuse-ld from bfd to lld must change effective backend and link fingerprint"
    );

    let inv1 = plan1.resolved_invocation.as_ref().unwrap();
    let inv2 = plan2.resolved_invocation.as_ref().unwrap();
    assert_ne!(inv1.backend_path, inv2.backend_path);
    assert_ne!(inv1.backend_fingerprint, inv2.backend_fingerprint);
}

#[test]
fn test_4_changing_dash_b_updates_or_invalidates_backend_and_fingerprint() {
    let dir = tempdir().unwrap();
    let cid = dummy_compiler_id();
    let target_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();

    let fake_rt = dir.path().join("libvanta_rt.a");
    fs::write(&fake_rt, b"FAKE_RUNTIME_ARCHIVE").unwrap();

    let resolver = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"GCC_DRIVER", None)
        .with_flag_downstream(
            "gcc",
            "-B/opt/toolchain-v1/",
            Ok(Some(PathBuf::from("/opt/toolchain-v1/ld"))),
        )
        .with_flag_downstream(
            "gcc",
            "-B/opt/toolchain-v2/",
            Ok(Some(PathBuf::from("/opt/toolchain-v2/ld"))),
        )
        .with_flag_downstream(
            "gcc",
            "-B/broken/",
            Err("Toolchain prefix /broken/ has no valid linker".into()),
        )
        .with_executable("ld", "/opt/toolchain-v1/ld", b"V1_LD_BYTES", None)
        .with_executable("ld", "/opt/toolchain-v2/ld", b"V2_LD_BYTES", None);

    let mut opts1 = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts1.linker = LinkerKind::System;
    opts1.runtime_path = Some(fake_rt.clone());
    opts1.linker_flags = vec!["-B/opt/toolchain-v1/".into()];

    let mut opts2 = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts2.linker = LinkerKind::System;
    opts2.runtime_path = Some(fake_rt.clone());
    opts2.linker_flags = vec!["-B/opt/toolchain-v2/".into()];

    let mut opts_broken = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts_broken.linker = LinkerKind::System;
    opts_broken.runtime_path = Some(fake_rt);
    opts_broken.linker_flags = vec!["-B/broken/".into()];

    let plan1 =
        CompilerSession::resolve_link_plan_with_resolver(&opts1, &target_spec, &resolver).unwrap();
    let plan2 =
        CompilerSession::resolve_link_plan_with_resolver(&opts2, &target_spec, &resolver).unwrap();

    assert_eq!(
        plan1.downstream_linker.as_ref().unwrap().path,
        Path::new("/opt/toolchain-v1/ld")
    );
    assert_eq!(
        plan2.downstream_linker.as_ref().unwrap().path,
        Path::new("/opt/toolchain-v2/ld")
    );

    let fp1 = plan1.fingerprint(&cid, &[]).unwrap();
    let fp2 = plan2.fingerprint(&cid, &[]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Changing -B prefix must update effective backend and fingerprint"
    );

    // Broken -B flag must fail closed during resolution
    let broken_res =
        CompilerSession::resolve_link_plan_with_resolver(&opts_broken, &target_spec, &resolver);
    assert!(
        broken_res.is_err(),
        "Invalid -B flag causing backend resolution failure must return Err"
    );
}

#[test]
fn test_5_exact_resolved_backend_path_is_backend_used_by_actual_invocation() {
    let dir = tempdir().unwrap();
    let target_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();

    let fake_rt = dir.path().join("libvanta_rt.a");
    fs::write(&fake_rt, b"FAKE_RUNTIME_ARCHIVE").unwrap();

    let resolver = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"GCC_DRIVER", None)
        .with_downstream("gcc", None, Ok(Some(PathBuf::from("/usr/bin/ld"))))
        .with_executable("ld", "/usr/bin/ld", b"GNU_LD_BYTES", None);

    let mut opts = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts.linker = LinkerKind::System;
    opts.runtime_path = Some(fake_rt);

    let plan =
        CompilerSession::resolve_link_plan_with_resolver(&opts, &target_spec, &resolver).unwrap();

    let res = CompilerSession::link_target_executable_with_plan_and_resolver(
        &opts,
        &target_spec,
        b"OBJ_BYTES",
        &plan,
        &resolver,
    );

    assert!(
        res.is_ok(),
        "Link execution should succeed: {:?}",
        res.err()
    );

    let recorded = resolver.get_executed_links();
    assert_eq!(
        recorded.len(),
        1,
        "Exactly one link execution should be performed"
    );
    let execution: &MockExecution = &recorded[0];

    assert_eq!(execution.driver_path, Path::new("/usr/bin/gcc"));
    assert_eq!(
        execution.expected_backend,
        Some(PathBuf::from("/usr/bin/ld")),
        "The expected backend passed to execution must match the resolved downstream linker"
    );
    assert!(
        execution.args.contains(&"-o".to_string()),
        "Execution args must contain -o flag"
    );
}

#[test]
fn test_6_replacing_backend_between_resolution_and_execution_aborts_with_hash_mismatch() {
    let dir = tempdir().unwrap();
    let target_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();

    let fake_rt = dir.path().join("libvanta_rt.a");
    fs::write(&fake_rt, b"FAKE_RUNTIME_ARCHIVE").unwrap();

    let mut resolver = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"GCC_DRIVER", None)
        .with_downstream("gcc", None, Ok(Some(PathBuf::from("/usr/bin/ld"))))
        .with_executable("ld", "/usr/bin/ld", b"ORIGINAL_LD_BYTES_LEN20", None);

    let mut opts = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts.linker = LinkerKind::System;
    opts.runtime_path = Some(fake_rt);

    let plan =
        CompilerSession::resolve_link_plan_with_resolver(&opts, &target_spec, &resolver).unwrap();

    // Simulate tampering or hot-swap of the backend binary on disk between resolution and execution
    resolver = resolver.with_executable("ld", "/usr/bin/ld", b"TAMPERED_LD_BYTES_LEN20", None);

    let res = CompilerSession::link_target_executable_with_plan_and_resolver(
        &opts,
        &target_spec,
        b"OBJ_BYTES",
        &plan,
        &resolver,
    );

    assert!(
        res.is_err(),
        "Modifying backend executable content between resolution and execution must abort"
    );
    let err = res.unwrap_err();
    assert!(
        err.contains("Pre-link executable integrity check failed")
            && err.contains("content modified between resolution and execution"),
        "Error message must clearly diagnose content modification: {}",
        err
    );

    // Verify zero link executions occurred
    assert_eq!(
        resolver.get_executed_links().len(),
        0,
        "Link execution must be aborted before spawning driver"
    );
}

#[test]
fn test_7_driver_untrustworthy_backend_fails_closed() {
    let dir = tempdir().unwrap();
    let target_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();

    let fake_rt = dir.path().join("libvanta_rt.a");
    fs::write(&fake_rt, b"FAKE_RUNTIME_ARCHIVE").unwrap();

    // Driver returns an error when queried for downstream linker
    let resolver_err = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"GCC_DRIVER", None)
        .with_downstream(
            "gcc",
            None,
            Err("Driver -print-prog-name=ld exited with code 1: internal error".into()),
        );

    let mut opts = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts.linker = LinkerKind::System;
    opts.runtime_path = Some(fake_rt);

    let res = CompilerSession::resolve_link_plan_with_resolver(&opts, &target_spec, &resolver_err);

    assert!(
        res.is_err(),
        "Driver that cannot authoritatively provide downstream linker must fail closed"
    );
    assert!(res.unwrap_err().contains("internal error"));
}

#[test]
fn test_8_identical_resolved_invocations_produce_deterministic_fingerprints() {
    let dir = tempdir().unwrap();
    let cid = dummy_compiler_id();
    let target_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();

    let fake_rt = dir.path().join("libvanta_rt.a");
    fs::write(&fake_rt, b"FAKE_RUNTIME_ARCHIVE").unwrap();

    let resolver = MockLinkerResolver::new()
        .with_executable("gcc", "/usr/bin/gcc", b"DETERMINISTIC_GCC", None)
        .with_downstream("gcc", None, Ok(Some(PathBuf::from("/usr/bin/ld"))))
        .with_executable("ld", "/usr/bin/ld", b"DETERMINISTIC_LD", None);

    let mut opts = CompilerOptions::new(dir.path().join("main.va"), dir.path().join("app"));
    opts.linker = LinkerKind::System;
    opts.runtime_path = Some(fake_rt);

    let plan1 =
        CompilerSession::resolve_link_plan_with_resolver(&opts, &target_spec, &resolver).unwrap();
    let plan2 =
        CompilerSession::resolve_link_plan_with_resolver(&opts, &target_spec, &resolver).unwrap();

    assert_eq!(plan1.resolved_invocation, plan2.resolved_invocation);

    let fp1 = plan1.fingerprint(&cid, &[]).unwrap();
    let fp2 = plan2.fingerprint(&cid, &[]).unwrap();

    assert_eq!(
        fp1, fp2,
        "Identical resolved invocations must produce bit-for-bit deterministic fingerprints"
    );
}

#[test]
fn test_9_internal_linking_works_without_resolving_external_linker_binaries() {
    let dir = tempdir().unwrap();
    let cid = dummy_compiler_id();
    let target_spec = TargetTriple::X86_64UnknownLinuxGnu.spec();

    // Resolver has zero executables registered
    let empty_resolver = MockLinkerResolver::new();

    let src_path = dir.path().join("main.va");
    let exe_path = dir.path().join("main.out");
    fs::write(&src_path, "fn main() -> i64 { return 42; }\n").unwrap();

    let mut opts = CompilerOptions::new(src_path.clone(), exe_path.clone());
    opts.linker = LinkerKind::Internal;

    let plan =
        CompilerSession::resolve_link_plan_with_resolver(&opts, &target_spec, &empty_resolver)
            .expect("Internal linking plan resolution must succeed without external executables");

    assert!(plan.linker_driver.is_none());
    assert!(plan.downstream_linker.is_none());
    assert!(plan.resolved_invocation.is_none());

    let fp = plan.fingerprint(&cid, &[]);
    assert!(
        fp.is_ok(),
        "Internal link fingerprint should compute successfully: {:?}",
        fp.err()
    );

    // Full compilation and execution using internal pure-Rust linker
    let mut session = CompilerSession::new();
    let compile_res = session.compile(&opts);
    assert!(
        compile_res.is_ok(),
        "Internal linker must compile successfully: {:?}",
        compile_res.err()
    );
    assert!(exe_path.exists(), "Internal linker must produce executable");

    let output = std::process::Command::new(&exe_path).output().unwrap();
    assert_eq!(output.status.code().unwrap_or(-1), 42);
}

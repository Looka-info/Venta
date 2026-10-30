use std::fs;
use std::path::Path;
use tempfile::tempdir;
use vanta_build::cache::{ArtifactType, CacheManager, LinkRecord};
use vanta_build::engine::{IncrementalBuildEngine, IncrementalOptions};
use vanta_build::fingerprint::{
    CompilerIdentity, ContentFingerprint, Domain, LinkConfiguration, LinkInputKind,
    ResolvedLibrary, ResolvedLinkInput,
};

fn dummy_compiler_id() -> CompilerIdentity {
    CompilerIdentity {
        compiler_version: "2.1.0".into(),
        cache_schema_version: 1,
        git_revision: "rev123".into(),
        target_triple: "x86_64-unknown-linux-gnu".into(),
        source_digest: "1111111111111111111111111111111111111111111111111111111111111111".into(),
    }
}

#[test]
fn test_1_changing_linker_kind_invalidates_fingerprint() {
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");

    let cfg1 = LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    let mut cfg2 = cfg1.clone();
    cfg2.linker_kind = "system".into();

    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Changing linker kind (internal vs system) must invalidate link fingerprint"
    );
}

#[test]
fn test_2_changing_linker_option_invalidates_fingerprint() {
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");

    let mut cfg1 = LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "system", "bin/app");
    cfg1.linker_flags = vec!["-no-pie".into()];

    let mut cfg2 = cfg1.clone();
    cfg2.linker_flags = vec!["-pie".into()];

    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Changing linker flags (-no-pie vs -pie) must invalidate link fingerprint"
    );
}

#[test]
fn test_3_changing_requested_library_names_invalidates_fingerprint() {
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");

    let mut cfg1 =
        LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    cfg1.requested_libraries = vec!["m".into(), "c".into()];

    let mut cfg2 = cfg1.clone();
    cfg2.requested_libraries = vec!["m".into(), "pthread".into()];

    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Changing requested libraries must invalidate link fingerprint"
    );
}

#[test]
fn test_4_changing_library_search_path_resolution_invalidates_fingerprint() {
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");
    let dir = tempdir().unwrap();

    let dir_a = dir.path().join("dir_a");
    let dir_b = dir.path().join("dir_b");
    fs::create_dir_all(&dir_a).unwrap();
    fs::create_dir_all(&dir_b).unwrap();

    let lib_a_path = dir_a.join("libfoo.a");
    let lib_b_path = dir_b.join("libfoo.a");
    fs::write(&lib_a_path, b"LIB_FOO_VERSION_A").unwrap();
    fs::write(&lib_b_path, b"LIB_FOO_VERSION_B").unwrap();

    let mut cfg1 =
        LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    cfg1.library_search_paths = vec![dir_a.clone()];
    cfg1.resolved_libraries = vec![ResolvedLibrary {
        name: "foo".into(),
        resolved_path: Some(lib_a_path.clone()),
        input: Some(
            ResolvedLinkInput::from_path(LinkInputKind::ResolvedLibrary, &lib_a_path).unwrap(),
        ),
    }];

    let mut cfg2 =
        LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    cfg2.library_search_paths = vec![dir_b.clone()];
    cfg2.resolved_libraries = vec![ResolvedLibrary {
        name: "foo".into(),
        resolved_path: Some(lib_b_path.clone()),
        input: Some(
            ResolvedLinkInput::from_path(LinkInputKind::ResolvedLibrary, &lib_b_path).unwrap(),
        ),
    }];

    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Changing library resolution across search paths must invalidate link fingerprint"
    );
}

#[test]
fn test_5_changing_resolved_library_contents_invalidates_fingerprint() {
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");
    let dir = tempdir().unwrap();

    let lib_path = dir.path().join("libbar.a");
    fs::write(&lib_path, b"LIBRARY_CONTENT_VERSION_1_SAME_LEN_32_BYTES").unwrap();

    let mut cfg1 =
        LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    cfg1.resolved_libraries = vec![ResolvedLibrary {
        name: "bar".into(),
        resolved_path: Some(lib_path.clone()),
        input: Some(
            ResolvedLinkInput::from_path(LinkInputKind::ResolvedLibrary, &lib_path).unwrap(),
        ),
    }];
    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();

    // Overwrite with identical length but different content
    fs::write(&lib_path, b"LIBRARY_CONTENT_VERSION_2_SAME_LEN_32_BYTES").unwrap();

    let mut cfg2 =
        LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    cfg2.resolved_libraries = vec![ResolvedLibrary {
        name: "bar".into(),
        resolved_path: Some(lib_path.clone()),
        input: Some(
            ResolvedLinkInput::from_path(LinkInputKind::ResolvedLibrary, &lib_path).unwrap(),
        ),
    }];
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Modifying library contents of identical length must invalidate link fingerprint"
    );
}

#[test]
fn test_6_replacing_runtime_archive_with_different_same_length_contents_invalidates_fingerprint() {
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");
    let dir = tempdir().unwrap();

    let rt_path = dir.path().join("libvanta_rt.a");
    fs::write(&rt_path, b"RUNTIME_CONTENT_V1_SAME_LEN_32_BYTES").unwrap();

    let mut cfg1 = LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "system", "bin/app");
    cfg1.runtime_archive =
        Some(ResolvedLinkInput::from_path(LinkInputKind::RuntimeArchive, &rt_path).unwrap());
    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();

    // Overwrite with same byte size, different content
    fs::write(&rt_path, b"RUNTIME_CONTENT_V2_SAME_LEN_32_BYTES").unwrap();

    let mut cfg2 = LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "system", "bin/app");
    cfg2.runtime_archive =
        Some(ResolvedLinkInput::from_path(LinkInputKind::RuntimeArchive, &rt_path).unwrap());
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Replacing runtime archive with different same-length contents must invalidate link fingerprint"
    );
}

#[test]
fn test_7_replacing_additional_object_with_different_same_length_contents_invalidates_fingerprint()
{
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");
    let dir = tempdir().unwrap();

    let obj_path = dir.path().join("extra.o");
    fs::write(&obj_path, b"EXTRA_OBJ_CONTENT_V1_SAME_LEN_32_BY").unwrap();

    let mut cfg1 =
        LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    cfg1.additional_objects =
        vec![ResolvedLinkInput::from_path(LinkInputKind::AdditionalObject, &obj_path).unwrap()];
    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();

    // Overwrite with same byte size, different content
    fs::write(&obj_path, b"EXTRA_OBJ_CONTENT_V2_SAME_LEN_32_BY").unwrap();

    let mut cfg2 =
        LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    cfg2.additional_objects =
        vec![ResolvedLinkInput::from_path(LinkInputKind::AdditionalObject, &obj_path).unwrap()];
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Replacing additional object with different same-length contents must invalidate link fingerprint"
    );
}

#[test]
fn test_8_changing_effective_system_linker_identity_invalidates_fingerprint() {
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");

    let mut cfg1 = LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "system", "bin/app");
    cfg1.linker_path = Some("/usr/bin/gcc-11".into());
    cfg1.linker_version = Some("gcc (Ubuntu 11.4.0-1ubuntu1~22.04) 11.4.0".into());

    let mut cfg2 = cfg1.clone();
    cfg2.linker_path = Some("/usr/bin/gcc-12".into());
    cfg2.linker_version = Some("gcc (Ubuntu 12.3.0-1ubuntu1~22.04) 12.3.0".into());

    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Changing system linker identity (path/version) must invalidate link fingerprint"
    );
}

#[test]
fn test_9_changing_map_file_configuration_invalidates_fingerprint() {
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");

    let mut cfg1 =
        LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    cfg1.map_file = None;

    let mut cfg2 = cfg1.clone();
    cfg2.map_file = Some("output.map".into());

    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_ne!(
        fp1, fp2,
        "Changing map-file configuration must invalidate link fingerprint"
    );
}

#[test]
fn test_10_unreadable_archive_returns_error_instead_of_valid_fingerprint() {
    let non_existent = Path::new("nonexistent/static/archive.a");
    let result = ResolvedLinkInput::from_path(LinkInputKind::StaticArchive, non_existent);
    assert!(
        result.is_err(),
        "ResolvedLinkInput::from_path must fail closed on missing archive"
    );

    let engine = IncrementalBuildEngine::new(IncrementalOptions::default()).unwrap();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"obj");
    let engine_result =
        engine.compute_link_fingerprint(&[obj_fp], &[non_existent.to_path_buf()], &[]);
    assert!(
        engine_result.is_err(),
        "compute_link_fingerprint must return Err when archive cannot be read"
    );
}

#[test]
fn test_11_unreadable_additional_input_returns_error() {
    let non_existent = Path::new("nonexistent/extra/input.o");
    let result = ResolvedLinkInput::from_path(LinkInputKind::AdditionalObject, non_existent);
    assert!(
        result.is_err(),
        "ResolvedLinkInput::from_path must fail closed on missing additional input"
    );

    let engine = IncrementalBuildEngine::new(IncrementalOptions::default()).unwrap();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"obj");
    let engine_result =
        engine.compute_link_fingerprint(&[obj_fp], &[], &[non_existent.to_path_buf()]);
    assert!(
        engine_result.is_err(),
        "compute_link_fingerprint must return Err when additional input cannot be read"
    );
}

#[test]
fn test_12_identical_effective_link_plans_produce_identical_fingerprints() {
    let cid = dummy_compiler_id();
    let obj_fp = ContentFingerprint::for_bytes(Domain::Object, b"main_object");
    let dir = tempdir().unwrap();

    let arch_path = dir.path().join("libcommon.a");
    fs::write(&arch_path, b"COMMON_ARCHIVE_DATA").unwrap();

    let mut cfg1 =
        LinkConfiguration::new("x86_64-unknown-linux-gnu", "elf64", "internal", "bin/app");
    cfg1.static_archives =
        vec![ResolvedLinkInput::from_path(LinkInputKind::StaticArchive, &arch_path).unwrap()];
    cfg1.requested_libraries = vec!["c".into()];

    let cfg2 = cfg1.clone();

    let fp1 = cfg1.fingerprint(&cid, &[obj_fp]).unwrap();
    let fp2 = cfg2.fingerprint(&cid, &[obj_fp]).unwrap();

    assert_eq!(
        fp1, fp2,
        "Identical effective link plans must produce identical fingerprints"
    );
}

#[test]
fn test_13_cached_link_record_verification() {
    let dir = tempdir().unwrap();
    let cache_dir = dir.path().join("cache");
    let exe_path = dir.path().join("test_app");
    fs::write(&exe_path, b"FAKE_EXECUTABLE_BINARY_DATA").unwrap();

    let cache = CacheManager::new(cache_dir, "x86_64-unknown-linux-gnu").unwrap();
    let link_key = ContentFingerprint::for_bytes(Domain::Link, b"link_key_1");
    let exe_checksum = ContentFingerprint::for_bytes(Domain::Link, b"FAKE_EXECUTABLE_BINARY_DATA");

    let record = LinkRecord {
        exe_path: exe_path.to_string_lossy().to_string(),
        exe_size: 27,
        exe_checksum,
        link_key,
        compiler_id: cache.compiler_identity().fingerprint().to_hex(),
    };
    let record_bytes = serde_json::to_vec(&record).unwrap();
    cache
        .store_artifact(&link_key, ArtifactType::LinkExecutable, &record_bytes, &[])
        .unwrap();

    // 1. Valid record and valid file -> reusable
    assert!(cache.get_cached_link(&link_key, &exe_path));

    // 2. Different link_key -> miss
    let wrong_key = ContentFingerprint::for_bytes(Domain::Link, b"link_key_2");
    assert!(!cache.get_cached_link(&wrong_key, &exe_path));

    // 3. Deleted executable -> miss
    fs::remove_file(&exe_path).unwrap();
    assert!(!cache.get_cached_link(&link_key, &exe_path));

    // 4. Corrupted checksum -> miss
    fs::write(&exe_path, b"TAMPERED_EXECUTABLE_BINARY_").unwrap(); // same 27 bytes, different content
    assert!(!cache.get_cached_link(&link_key, &exe_path));
}

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_hello_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("hello");
    fs::create_dir_all(project.join("src")).expect("create project src");
    copy_fixture("axiom.toml", &project.join("axiom.toml"));
    copy_fixture("axiom.lock", &project.join("axiom.lock"));
    copy_fixture("src/main.ax", &project.join("src/main.ax"));

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["packages"][0]["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    assert!(payload["generated_rust"].is_null());
    assert!(Path::new(binary).exists(), "cranelift binary exists");
    assert!(
        Path::new(binary).with_extension("cranelift.o").exists(),
        "cranelift object exists"
    );

    let run = Command::new(binary).output().expect("run cranelift binary");
    assert!(
        run.status.success(),
        "cranelift binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "hello from stage1\n42\ntrue\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_scalar_aggregate_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("scalar-aggregate");
    write_scalar_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift scalar build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift scalar binary");
    assert!(
        run.status.success(),
        "cranelift scalar binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "native\n7\n12\n10\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_numeric_cross_width_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("numeric-cross-width");
    write_numeric_cross_width_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift numeric build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift numeric binary");
    assert!(
        run.status.success(),
        "cranelift numeric binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "7\n255\n3\n-4\n42\n255\n44\n-126\n18446744073709551615\n18446744073709551615\n255\n0\n16777216\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_const_sized_array_conformance_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("const-sized-array");
    copy_conformance_fixture("const_sized_arrays", &project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift const-sized-array build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift const-sized-array binary");
    assert!(
        run.status.success(),
        "cranelift const-sized-array binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n6\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_static_scalar_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("static-scalar-globals");
    write_static_scalar_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift static scalar build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift static scalar binary");
    assert!(
        run.status.success(),
        "cranelift static scalar binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "hello static\n42\n43\ntrue\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_struct_field_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("struct-field");
    write_struct_field_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift struct build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift struct binary");
    assert!(
        run.status.success(),
        "cranelift struct binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "3\ntrue\nstage1 structs\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_enum_match_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("enum-match");
    write_enum_match_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift enum match build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift enum match binary");
    assert!(
        run.status.success(),
        "cranelift enum match binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "multi\nnamed\npayload\n2\n8\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_result_helpers_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("result-helpers");
    write_result_helpers_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift result-helpers build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift result-helpers binary");
    assert!(
        run.status.success(),
        "cranelift result-helpers binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true\ntrue\n7\n9\nbuilt\nboom\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_array_helpers_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("array-helpers");
    write_array_helpers_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift array-helpers build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift array-helpers binary");
    assert!(
        run.status.success(),
        "cranelift array-helpers binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n10\n30\n40\n");
}

#[test]
fn cranelift_backend_rejects_process_status_binary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("process-status");
    write_process_status_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift process-status build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("unsupported by --backend cranelift spike"),
        "expected backend rejection for process_status, got: {combined}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_borrowed_slice_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("borrowed-slice");
    write_borrowed_slice_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift borrowed-slice build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift borrowed-slice binary");
    assert!(
        run.status.success(),
        "cranelift borrowed-slice binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n4\n8\n6\n3\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_owned_move_state_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("owned-move-state");
    write_owned_move_state_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift owned move-state build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift owned move-state binary");
    assert!(
        run.status.success(),
        "cranelift owned move-state binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\nleft\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_debug_build_emits_sidecars_without_axiom_dwarf() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("hello-debug");
    fs::create_dir_all(project.join("src")).expect("create project src");
    copy_fixture("axiom.toml", &project.join("axiom.toml"));
    copy_fixture("axiom.lock", &project.join("axiom.lock"));
    copy_fixture("src/main.ax", &project.join("src/main.ax"));

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--debug",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift --debug");
    assert!(
        output.status.success(),
        "cranelift debug build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    assert_eq!(payload["debug"], true);
    assert!(payload["generated_rust"].is_null());
    let binary = payload["binary"].as_str().expect("binary path");
    let debug_map = payload["debug_map"].as_str().expect("debug map path");
    let debug_manifest = payload["debug_manifest"]
        .as_str()
        .expect("debug manifest path");
    assert!(Path::new(binary).exists(), "cranelift binary exists");
    assert!(Path::new(debug_map).exists(), "debug map exists");
    assert!(Path::new(debug_manifest).exists(), "debug manifest exists");

    let source = project
        .join("src/main.ax")
        .canonicalize()
        .expect("canonical source path")
        .display()
        .to_string();
    let map: Value =
        serde_json::from_str(&fs::read_to_string(debug_map).expect("read cranelift debug map"))
            .expect("parse cranelift debug map");
    assert_eq!(
        map["schema_version"],
        "axiom.stage1.direct_native.debug_map.v1"
    );
    assert_eq!(map["backend"], "cranelift");
    assert_eq!(map["binary"], binary);
    assert!(
        map["source_spans"]
            .as_array()
            .expect("direct-native debug source spans")
            .iter()
            .any(|span| span["source"] == source),
        "direct-native debug map should retain Axiom source spans for cranelift builds"
    );

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(debug_manifest).expect("read cranelift debug manifest"),
    )
    .expect("parse cranelift debug manifest");
    assert_eq!(
        manifest["schema_version"],
        "axiom.stage1.direct_native.debug_manifest.v1"
    );
    assert_eq!(manifest["artifact_class"], "native_binary");
    assert_eq!(manifest["backend"], "cranelift");
    assert_eq!(manifest["binary"], binary);
    assert!(manifest.get("generated_rust").is_none());
    assert!(manifest.get("generated_rust_hash").is_none());
    assert_eq!(manifest["debug_map"], debug_map);
    assert!(
        manifest["source_files"]
            .as_array()
            .expect("direct-native manifest source files")
            .iter()
            .any(|file| file["path"] == source),
        "direct-native debug manifest should retain Axiom source files"
    );
    assert_eq!(manifest["native_debug"]["producer"], "cranelift");
    assert_eq!(manifest["native_debug"]["debuginfo"], 0);
    assert_eq!(manifest["native_debug"]["opt_level"], 0);
    assert_eq!(manifest["native_debug"]["axiom_dwarf"], false);
    assert!(
        manifest["native_debug"]["native_debug_info"]
            .as_str()
            .expect("native debug info")
            .contains("does not emit native Axiom DWARF yet")
    );
    assert!(
        manifest.get("rustc").is_none(),
        "cranelift debug manifests should not claim rustc debug settings"
    );
    assert!(
        manifest["source_files"]
            .as_array()
            .expect("source files")
            .iter()
            .any(|source_file| source_file["path"] == source
                && source_file["mapping_count"].as_u64().unwrap_or(0) > 0),
        "debug manifest should count Axiom source mappings for cranelift builds"
    );

    let run = Command::new(binary)
        .output()
        .expect("run cranelift debug binary");
    assert!(
        run.status.success(),
        "cranelift debug binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "hello from stage1
42
true
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_map_index_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("map-index");
    write_map_index_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift map build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift map binary");
    assert!(
        run.status.success(),
        "cranelift map binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "11
true
false
high
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_crypto_hash_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-hash");
    write_crypto_hash_project(&project, true);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift crypto hash build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift crypto hash binary");
    assert!(
        run.status.success(),
        "cranelift crypto hash binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_sync_primitives_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("sync-primitives");
    write_sync_primitives_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift sync primitives build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift sync primitives binary");
    assert!(
        run.status.success(),
        "cranelift sync primitives binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "2
true
empty
message
"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_json_serdes_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("json-serdes");
    write_json_serdes_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift json build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift json binary");
    assert!(
        run.status.success(),
        "cranelift json binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        r#"42
false
"hello"
42
true
{"name":"axiom","count":3,"ready":true}
axiom
3
true
{"name":"axiom","count":3,"ready":true}
"axiom"
no int
"#,
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_logging_stdio_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("logging-stdio");
    write_logging_stdio_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift logging stdio build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift logging stdio binary");
    assert!(
        run.status.success(),
        "cranelift logging stdio binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "true\n");
    assert_eq!(String::from_utf8_lossy(&run.stderr), "hello stderr\n");
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_rejects_float_map_keys() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("float-map-key");
    write_float_map_key_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift float-keyed map build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("map float keys are not supported by the cranelift spike"),
        "expected float map key diagnostic, got: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_crypto_hash_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-hash-denied");
    write_crypto_hash_project(&project, false);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift crypto hash denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].crypto = true"),
        "expected crypto capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_process_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("process-denied");
    write_process_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift process-denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].process = true"),
        "expected process capability diagnostic, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial must happen before backend lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_http_client_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-client-denied");
    write_http_client_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift http client denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].net = true"),
        "expected net capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_rejects_nonzero_clock_sleep() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock-nonzero-sleep");
    write_clock_nonzero_sleep_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift nonzero clock sleep build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("nonzero clock_sleep_ms is not supported by the cranelift spike"),
        "expected nonzero sleep rejection, got: {combined}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_clock_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock");
    write_clock_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift clock build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift clock binary");
    assert!(
        run.status.success(),
        "cranelift clock binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true
true
true
true
"
    );
}

#[test]
fn cranelift_backend_rejects_clock_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock-denied");
    write_clock_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift clock-denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].clock = true"),
        "expected clock capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "clock capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_capability_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-denied");
    write_fs_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift fs-denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].fs = true"),
        "expected capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_tcp_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("tcp-denied");
    write_tcp_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift tcp denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].net = true"),
        "expected net capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_udp_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("udp-denied");
    write_udp_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift udp denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].net = true"),
        "expected net capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_net_resolve_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("net-resolve-denied");
    write_net_resolve_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift net resolve denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].net = true"),
        "expected net capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_fs_write_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("fs-write-denied");
    write_fs_write_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift fs-write denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].fs:write = true"),
        "expected fs:write capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[cfg(not(windows))]
#[test]
fn cranelift_backend_builds_env_read_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("env-read");
    write_env_read_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .env("AXIOM_CRANELIFT_ENV_READ", "native-env")
        .env_remove("__AXIOM_CRANELIFT_ENV_MISSING__")
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");
    assert!(
        output.status.success(),
        "cranelift env build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift env binary");
    assert!(
        run.status.success(),
        "cranelift env binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "native-env\nmissing\n"
    );
}

#[test]
fn cranelift_backend_rejects_env_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("env-denied");
    write_env_denial_project(&project);

    let output = Command::new(env!("CARGO_BIN_EXE_axiomc"))
        .args([
            "build",
            project.to_str().expect("project path"),
            "--backend",
            "cranelift",
            "--json",
        ])
        .output()
        .expect("run axiomc build --backend cranelift");

    assert!(
        !output.status.success(),
        "cranelift env-denied build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].env"),
        "expected env capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "env capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}


fn copy_fixture(relative: &str, destination: &Path) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hello")
        .join(relative);
    fs::copy(&fixture, destination).unwrap_or_else(|err| {
        panic!(
            "copy fixture {} to {}: {err}",
            fixture.display(),
            destination.display()
        )
    });
}

fn copy_conformance_fixture(fixture_name: &str, destination: &Path) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/pass")
        .join(fixture_name);
    fs::create_dir_all(destination.join("src")).expect("create conformance project src");
    for relative in ["axiom.toml", "axiom.lock", "src/main_test.ax"] {
        let source = fixture.join(relative);
        let target = destination.join(relative);
        fs::copy(&source, &target).unwrap_or_else(|err| {
            panic!(
                "copy fixture {} to {}: {err}",
                source.display(),
                target.display()
            )
        });
    }
}

fn write_scalar_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create scalar project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-scalar-aggregate\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write scalar manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-scalar-aggregate\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write scalar lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn sum_tail(values: [int; 3]): int {\nreturn values[1] * values[2]\n}\n\nfn adjust(value: int): int {\nreturn value / 2\n}\n\nlet label: (string, int) = (\"native\", 7)\nlet values: [int; 3] = [2, 3, 4]\nprint label.0\nprint label.1\nprint sum_tail(values)\nprint adjust(20)\n",
    )
    .expect("write scalar source");
}

fn write_numeric_cross_width_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create numeric project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-numeric-cross-width\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write numeric manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-numeric-cross-width\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write numeric lockfile");
    fs::write(
        project.join("src/main.ax"),
        "let wide_signed: i64 = 7i64\nlet narrow_signed: i32 = wide_signed as i32\n\nlet byte: u8 = 255u8\nlet widened_unsigned: i32 = byte as i32\n\nlet signed32: i32 = 3i32\nlet float32: f32 = signed32 as f32\n\nlet float64: f64 = -4.75f64\nlet signed64: i64 = float64 as i64\n\nlet same: i32 = 42i32 as i32\n\nlet signed_to_unsigned: u8 = -1i64 as u8\nlet narrowed_unsigned: u8 = 300i64 as u8\nlet narrowed_signed: i8 = 130i64 as i8\nlet wrapped_int: int = 18446744073709551615u64 as int\nlet max_u64: u64 = 18446744073709551615u64\nlet saturated_float_unsigned: u8 = 300.0f64 as u8\nlet negative_float_unsigned: u8 = -1.0f64 as u8\nlet rounded_f32: f32 = 16777216f32 + 1f32\n\nprint narrow_signed\nprint widened_unsigned\nprint float32 as int\nprint signed64\nprint same\nprint signed_to_unsigned\nprint narrowed_unsigned\nprint narrowed_signed\nprint wrapped_int\nprint max_u64\nprint saturated_float_unsigned\nprint negative_float_unsigned\nprint rounded_f32 as int\n",
    )
    .expect("write numeric source");
}

fn write_static_scalar_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create static scalar project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-static-scalar\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write static scalar manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-static-scalar\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write static scalar lockfile");
    fs::write(
        project.join("src/main.ax"),
        "static GREETING: string = \"hello static\"\nstatic ANSWER: int = 42\nstatic READY: bool = true\n\nfn bump(value: int): int {\nreturn value + ANSWER\n}\n\nprint GREETING\nprint ANSWER\nprint bump(1)\nprint READY\n",
    )
    .expect("write static scalar source");
}

fn write_enum_match_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create enum match project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-enum-match\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write enum match manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-enum-match\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write enum match lockfile");
    fs::write(
        project.join("src/main.ax"),
        "enum Message {\nPair(int, string)\nJob { id: int, label: string }\nText(string)\n}\n\nenum Signal {\nRed\nYellow\nGreen\n}\n\nfn render(message: Message): string {\nmatch message {\nPair(count, label) {\nreturn label\n}\nJob { label, id } {\nreturn label\n}\nText(text) {\nreturn text\n}\n}\n}\n\nfn signal_priority(signal: Signal): int {\nreturn match signal { Red => 3, Yellow => 2, Green => 1 }\n}\n\nlet first: Message = Pair(7, \"multi\")\nlet second: Message = Job { id: 9, label: \"named\" }\nlet score: int = match Some(7) {\nSome(value) => value + 1\nNone => 0\n}\n\nprint render(first)\nprint render(second)\nprint render(Text(\"payload\"))\nprint signal_priority(Yellow)\nprint score\n",
    )
    .expect("write enum match source");
}

fn write_struct_field_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create struct project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-struct-field\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write struct manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-struct-field\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write struct lockfile");
    fs::write(
        project.join("src/main.ax"),
        "struct Pipeline {\nname: string\nsteps: int\nready: bool\n}\n\nfn label(pipeline: Pipeline): string {\nreturn pipeline.name\n}\n\nlet pipeline: Pipeline = Pipeline { name: \"stage1 structs\", steps: 3, ready: true }\nprint pipeline.steps\nprint pipeline.ready\nprint label(pipeline)\n",
    )
    .expect("write struct source");
}

fn write_result_helpers_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create result-helpers project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-result-helpers\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write result-helpers manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-result-helpers\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write result-helpers lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/outcome.ax\"\n\nstruct Payload {\nlabel: string\ncount: int\n}\n\nfn choose(flag: bool): Result<int, string> {\nif flag {\nreturn Ok(7)\n}\nreturn Err(\"boom\")\n}\n\nlet ok_predicate: Result<int, string> = choose(true)\nlet err_predicate: Result<int, string> = choose(false)\nlet ok_fallback: Result<int, string> = choose(true)\nlet err_fallback: Result<int, string> = choose(false)\nlet ok_match: Result<int, string> = choose(true)\nlet err_match: Result<int, string> = choose(false)\nprint result_is_ok<int, string>(ok_predicate)\nprint result_is_err<int, string>(err_predicate)\nprint result_unwrap_or<int, string>(ok_fallback, 1)\nprint result_unwrap_or<int, string>(err_fallback, 9)\nmatch ok_match {\nOk(value) {\nprint \"built\"\n}\nErr(message) {\nprint message\n}\n}\nmatch err_match {\nOk(value) {\nprint value\n}\nErr(message) {\nprint message\n}\n}\nlet payload: Result<Payload, string> = Ok(Payload { label: \"package\", count: 2 })\nmatch payload {\nOk(value) {\nlet total: int = value.count + 5\n}\nErr(message) {\nprint message\n}\n}\n",
    )
    .expect("write result-helpers source");
}

fn write_array_helpers_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create array-helpers project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-array-helpers\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write array-helpers manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-array-helpers\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write array-helpers lockfile");
    fs::write(
        project.join("src/main.ax"),
        "let values: [int; 3] = [10, 20, 30]\nprint len(values)\nprint first(values)\nprint last(values)\nprint first(values) + last(values)\n",
    )
    .expect("write array-helpers source");
}

fn write_borrowed_slice_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create borrowed-slice project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-borrowed-slice\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write borrowed-slice manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-borrowed-slice\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write borrowed-slice lockfile");
    fs::write(
        project.join("src/main.ax"),
        "fn tail(values: &[int]): &[int] {\nreturn values[1:]\n}\n\nlet values: [int] = [2, 4, 6, 8]\nlet window: &[int] = values[1:]\nprint len(window)\nprint first(window)\nprint last(window)\nprint window[1]\nlet nested: &[int] = tail(values[:])\nprint len(nested)\n",
    )
    .expect("write borrowed-slice source");
}

fn write_process_status_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create process-status project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-process-status\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = true\nunsafe_rationale = \"direct-native process-status regression executes deterministic system helpers\"\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write process-status manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-process-status\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write process-status lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/process.ax\"\nprint run_status(\"/usr/bin/true\")\nprint run_status(\"/usr/bin/false\")\n",
    )
    .expect("write process-status source");
}

fn write_owned_move_state_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create owned move project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-owned-move-state\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write owned move manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-owned-move-state\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write owned move lockfile");
    fs::write(
        project.join("src/main.ax"),
        "struct Pair {\nname: string\nvalues: [int]\n}\n\nlet pair: Pair = Pair { name: \"left\", values: [1, 2, 3] }\nlet moved: [int] = pair.values\nprint len(moved)\nprint pair.name\n",
    )
    .expect("write owned move source");
}

fn write_map_index_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create map project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-map-index\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write map manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-map-index\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write map lockfile");
    fs::write(
        project.join("src/main.ax"),
        "let scores: {string: int} = {\"build\": 7, \"deploy\": 9, \"deploy\": 11}\nprint scores[\"deploy\"]\n\nlet available: {string: int} = {\"build\": 7, \"deploy\": 9}\nprint map_contains_key<string, int>(available, \"build\")\n\nlet missing: {string: int} = {\"build\": 7, \"deploy\": 9}\nprint map_contains_key<string, int>(missing, \"test\")\n\nlet labels: {int: string} = {1: \"low\", 2: \"high\"}\nprint labels[2]\n",
    )
    .expect("write map source");
}

fn write_float_map_key_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create float map project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-float-map-key\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write float map manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-float-map-key\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write float map lockfile");
    fs::write(
        project.join("src/main.ax"),
        "let scores: {f64: int} = {1.5f64: 7}\nprint scores[1.5f64]\n",
    )
    .expect("write float map source");
}

fn write_crypto_hash_project(project: &Path, crypto: bool) {
    fs::create_dir_all(project.join("src")).expect("create crypto hash project src");
    fs::write(
        project.join("axiom.toml"),
        format!(
            "[package]\nname = \"cranelift-crypto-hash\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = {crypto}\n"
        ),
    )
    .expect("write crypto hash manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-crypto-hash\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write crypto hash lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/crypto_hash.ax\"\nprint sha256(\"abc\")\n",
    )
    .expect("write crypto hash source");
}

fn write_sync_primitives_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create sync primitives project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-sync-primitives"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write sync primitives manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-sync-primitives"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write sync primitives lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/sync.ax"

let counter: Mutex<int> = mutex<int>(1)
let guard: MutexGuard<int> = lock<int>(counter)
let updated: Mutex<int> = replace<int>(guard, 2)
let final_guard: MutexGuard<int> = lock<int>(updated)
print into_inner<int>(final_guard)

let ready: Once<string> = once_with<string>("configured")
print once_is_set<string>(ready)

let empty: Once<int> = once<int>(None)
match once_take<int>(empty) {
Some(value) {
print value
}
None {
print "empty"
}
}

let channel: Channel<string> = channel<string>(None)
let sent: Channel<string> = send<string>(channel, "message")
match try_recv<string>(sent) {
Some(message) {
print message
}
None {
print "missing"
}
}
"#,
    )
    .expect("write sync primitives source");
}

fn write_logging_stdio_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create logging stdio project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-logging-stdio"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write logging stdio manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-logging-stdio"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write logging stdio lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/io.ax"

let direct: int = eprintln("hello stderr")
print direct > 0
"#,
    )
    .expect("write logging stdio source");
}

fn write_json_serdes_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create json project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-json-serdes"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write json manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-json-serdes"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write json lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/json.ax"

fn doc(): string {
return object3(field_string("name", "axiom"), field_int("count", 3), field_bool("ready", true))
}

print stringify_int(42)
print stringify_bool(false)
print stringify_string("hello")

match parse_int(" 42 ") {
Some(value) {
print value
}
None {
print -1
}
}

match parse_bool("true") {
Some(value) {
print value
}
None {
print false
}
}

print doc()

match parse_field_string(doc(), "name") {
Some(value) {
print value
}
None {
print "missing name"
}
}

match parse_field_int(doc(), "count") {
Some(value) {
print value
}
None {
print -1
}
}

match parse_field_bool(doc(), "ready") {
Some(value) {
print value
}
None {
print false
}
}

match parse_value(doc()) {
Some(value) {
print stringify_value(value)
}
None {
print "invalid"
}
}

match parse_value(doc()) {
Some(value) {
match parse_field_value(value, "name") {
Some(name_value) {
print stringify_value(name_value)
}
None {
print "missing value"
}
}
}
None {
print "invalid"
}
}

match parse_int("nope") {
Some(value) {
print value
}
None {
print "no int"
}
}
"#,
    )
    .expect("write json source");
}
fn write_fs_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-fs-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write fs denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-fs-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write fs denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/fs.ax\"\nmatch read_file(\"src/fixture.txt\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\n",
    )
    .expect("write fs denied source");
}

fn write_http_client_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create http client denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-http-client-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write http client denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-http-client-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write http client denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/http.ax\"\nmatch get(\"http://127.0.0.1/\") {\nSome(_body) {\nprint true\n}\nNone {\nprint false\n}\n}\n",
    )
    .expect("write http client denied source");
}

fn write_tcp_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create tcp denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-tcp-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write tcp denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-tcp-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write tcp denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/net.ax\"\nmatch tcp_listen_loopback_once(\"pong\", 1000) {\nSome(_port) {\nprint true\n}\nNone {\nprint false\n}\n}\n",
    )
    .expect("write tcp denied source");
}

fn write_udp_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create udp denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-udp-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write udp denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-udp-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write udp denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/net.ax\"\nmatch udp_bind_loopback_once(\"pong\", 1000) {\nSome(_port) {\nprint true\n}\nNone {\nprint false\n}\n}\n",
    )
    .expect("write udp denied source");
}

fn write_net_resolve_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create net resolve denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-net-resolve-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write net resolve denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-net-resolve-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write net resolve denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/net.ax\"\nmatch resolve(\"localhost\") {\nSome(_address) {\nprint true\n}\nNone {\nprint false\n}\n}\n",
    )
    .expect("write net resolve denied source");
}

fn write_process_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create process denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-process-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write process denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-process-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write process denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/process.ax\"\nprint run_status(\"/usr/bin/true\")\n",
    )
    .expect("write process denied source");
}

fn write_fs_write_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create fs write denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-fs-write-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = true\n\"fs:write\" = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write fs write denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-fs-write-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write fs write denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/fs.ax\"\nprint write_file(\"out.txt\", \"content\")\n",
    )
    .expect("write fs write denied source");
}

fn write_env_read_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create env project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-env-read\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = true\nclock = false\ncrypto = false\n\n[unsafe_rationale]\nenv = \"Cranelift ABI regression covers direct-native env.read behavior for issue 928.\"\n",
    )
    .expect("write env manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-env-read\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write env lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/env.ax\"\nmatch get_env(\"AXIOM_CRANELIFT_ENV_READ\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing value\"\n}\n}\nmatch get_env(\"__AXIOM_CRANELIFT_ENV_MISSING__\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\n",
    )
    .expect("write env source");
}

fn write_env_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create env denied project src");
    fs::write(
        project.join("axiom.toml"),
        "[package]\nname = \"cranelift-env-denied\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.ax\"\nout_dir = \"dist\"\n\n[capabilities]\nfs = false\nnet = false\nprocess = false\nenv = false\nclock = false\ncrypto = false\n",
    )
    .expect("write env denied manifest");
    fs::write(
        project.join("axiom.lock"),
        "version = 1\n\n[[package]]\nname = \"cranelift-env-denied\"\nversion = \"0.1.0\"\nsource = \"path\"\n",
    )
    .expect("write env denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        "import \"std/env.ax\"\nmatch get_env(\"AXIOM_CRANELIFT_ENV_READ\") {\nSome(value) {\nprint value\n}\nNone {\nprint \"missing\"\n}\n}\n",
    )
    .expect("write env denied source");
}

fn write_clock_nonzero_sleep_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create clock nonzero sleep project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-clock-nonzero-sleep"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = true
crypto = false
"#,
    )
    .expect("write clock nonzero sleep manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-clock-nonzero-sleep"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write clock nonzero sleep lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/time.ax"
print sleep(duration_ms(1))
"#,
    )
    .expect("write clock nonzero sleep source");
}

fn write_clock_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create clock project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-clock"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = true
crypto = false
"#,
    )
    .expect("write clock manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-clock"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write clock lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/time.ax"
let start: Instant = now()
let pause: Duration = duration_ms(0)
print start.ms > 0
print now_ms() > 0
print sleep(pause) == 0
let elapsed: int = elapsed_ms(start)
print elapsed == elapsed
"#,
    )
    .expect("write clock source");
}

fn write_clock_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create clock denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-clock-denied"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = false
process = false
env = false
clock = false
crypto = false
"#,
    )
    .expect("write clock denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-clock-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write clock denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/time.ax"
let start: Instant = now()
print sleep(duration_ms(0))
print elapsed_ms(start)
"#,
    )
    .expect("write clock denied source");
}

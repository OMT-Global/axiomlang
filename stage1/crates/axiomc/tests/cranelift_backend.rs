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
    assert_eq!(payload["generated_rust"], Value::Null);
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
fn cranelift_backend_builds_regex_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("regex-surface");
    write_regex_project(&project);

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
        "cranelift regex build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift regex binary");
    assert!(
        run.status.success(),
        "cranelift regex binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true
238
issue-#-ready
xa
xaa
ba
"
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout.lines().nth(3),
        Some("xa"),
        "anchored replace_all must only rewrite the original leading match"
    );
    assert_eq!(stdout.lines().nth(4), Some("xaa"));
    assert_eq!(stdout.lines().nth(5), Some("ba"));
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
    let binary = payload["binary"].as_str().expect("binary path");
    assert_eq!(payload["generated_rust"], Value::Null);
    let debug_map = payload["debug_map"].as_str().expect("debug map path");
    let debug_manifest = payload["debug_manifest"]
        .as_str()
        .expect("debug manifest path");
    assert!(Path::new(binary).exists(), "cranelift binary exists");
    assert!(
        Path::new(binary).with_extension("cranelift.o").exists(),
        "cranelift object exists"
    );
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
            .expect("debug source spans")
            .iter()
            .any(|span| span["source"] == source),
        "debug map should retain Axiom source spans for cranelift builds"
    );

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(debug_manifest).expect("read cranelift debug manifest"),
    )
    .expect("parse cranelift debug manifest");
    assert_eq!(
        manifest["schema_version"],
        "axiom.stage1.direct_native.debug_manifest.v1"
    );
    assert_eq!(manifest["backend"], "cranelift");
    assert_eq!(manifest["artifact_class"], "native_binary");
    assert_eq!(manifest["binary"], binary);
    assert!(
        manifest["binary_hash"]
            .as_str()
            .is_some_and(|hash| !hash.is_empty()),
        "direct-native debug manifest should keep the binary hash in the integrity envelope"
    );
    assert_eq!(manifest["debug_map"], debug_map);
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
fn cranelift_backend_builds_crypto_mac_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-mac");
    write_crypto_mac_project(&project, true);

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
        "cranelift crypto mac build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse build JSON");
    assert_eq!(payload["backend"], "cranelift");
    let binary = payload["binary"].as_str().expect("binary path");
    let run = Command::new(binary)
        .output()
        .expect("run cranelift crypto mac binary");
    assert!(
        run.status.success(),
        "cranelift crypto mac binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8
164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737
true
true
false
false
true
false
"
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
fn cranelift_backend_builds_clock_binary() {
    if which::which("cc").is_err() {
        eprintln!("skipping cranelift backend smoke test because cc is unavailable");
        return;
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock");
    write_clock_project(&project, true, false);

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
        "true\ntrue\ntrue\ntrue\n"
    );
}

#[test]
fn cranelift_backend_rejects_clock_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock-denied");
    write_clock_project(&project, false, false);

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
        "cranelift clock denied build unexpectedly succeeded: stdout={} stderr={}",
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
fn cranelift_backend_rejects_nonzero_clock_sleep() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("clock-nonzero-sleep");
    write_clock_project(&project, true, true);

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
        "expected nonzero sleep guard, got: {combined}"
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
fn cranelift_backend_rejects_crypto_mac_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-mac-denied");
    write_crypto_mac_project(&project, false);

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
        "cranelift crypto mac denial build unexpectedly succeeded: stdout={} stderr={}",
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
fn cranelift_backend_rejects_crypto_random_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-random-denied");
    write_crypto_random_project(&project, false);

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
        "cranelift crypto random denial build unexpectedly succeeded: stdout={} stderr={}",
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
fn cranelift_backend_rejects_crypto_signature_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-signature-denied");
    write_crypto_signature_project(&project, false);

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
        "cranelift crypto signature denial build unexpectedly succeeded: stdout={} stderr={}",
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
fn cranelift_backend_rejects_crypto_aead_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("crypto-aead-denied");
    write_crypto_aead_project(&project, false);

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
        "cranelift crypto AEAD denial build unexpectedly succeeded: stdout={} stderr={}",
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

#[test]
fn cranelift_backend_rejects_ffi_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("ffi-denied");
    write_ffi_denial_project(&project);

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
        "cranelift ffi denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].ffi = true"),
        "expected ffi capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_http_server_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-server-denied");
    write_http_server_denial_project(&project);

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
        "cranelift http server denial build unexpectedly succeeded: stdout={} stderr={}",
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
fn cranelift_backend_rejects_async_runtime_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("async-runtime-denied");
    write_async_runtime_denial_project(&project);

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
        "cranelift async runtime denial build unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("requires [capabilities].async = true"),
        "expected async capability denial before backend lowering, got: {combined}"
    );
    assert!(
        !combined.contains("unsupported by --backend cranelift spike"),
        "capability denial should happen before cranelift unsupported-feature lowering: {combined}"
    );
}

#[test]
fn cranelift_backend_rejects_http_async_server_denial_before_backend_lowering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let project = temp.path().join("http-async-server-denied");
    write_http_async_server_denial_project(&project);

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
        "cranelift http async server denial build unexpectedly succeeded: stdout={} stderr={}",
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

fn write_ffi_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create ffi denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-ffi-denied"
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
ffi = false
"#,
    )
    .expect("write ffi denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-ffi-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write ffi denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"extern fn strlen(value: string): int from "c"
print strlen("hello")
"#,
    )
    .expect("write ffi denied source");
}

fn write_async_runtime_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create async runtime denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-async-runtime-denied"
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
async = false
"#,
    )
    .expect("write async runtime denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-async-runtime-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write async runtime denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/async.ax"
let task: Task<int> = ready<int>(1)
print await task
"#,
    )
    .expect("write async runtime denied source");
}

fn write_http_server_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create http server denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-http-server-denied"
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
    .expect("write http server denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-server-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http server denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/http.ax"
print serve_once("127.0.0.1:0", "ok")
"#,
    )
    .expect("write http server denied source");
}

fn write_http_async_server_denial_project(project: &Path) {
    fs::create_dir_all(project.join("src")).expect("create http async server denied project src");
    fs::write(
        project.join("axiom.toml"),
        r#"[package]
name = "cranelift-http-async-server-denied"
version = "0.1.0"

[build]
entry = "src/main.ax"
out_dir = "dist"

[capabilities]
fs = false
net = true
process = false
env = false
clock = false
crypto = false
async = false

[unsafe_rationale]
net = "Cranelift ABI regression covers async server capability denial ordering for issue 928."
"#,
    )
    .expect("write http async server denied manifest");
    fs::write(
        project.join("axiom.lock"),
        r#"version = 1

[[package]]
name = "cranelift-http-async-server-denied"
version = "0.1.0"
source = "path"
"#,
    )
    .expect("write http async server denied lockfile");
    fs::write(
        project.join("src/main.ax"),
        r#"import "std/http_async.ax"
let task: Task<bool> = async_serve_route(1, "/", "ok", 1)
print true
"#,
    )
    .expect("write http async server denied source");
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

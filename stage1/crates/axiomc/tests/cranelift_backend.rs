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
    let generated_rust = payload["generated_rust"]
        .as_str()
        .expect("generated Rust path");
    assert!(Path::new(binary).exists(), "cranelift binary exists");
    assert!(
        Path::new(generated_rust)
            .with_extension("cranelift.o")
            .exists(),
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

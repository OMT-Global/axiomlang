use axiomc::project::lower_project_to_mir;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn representative_examples_match_normalized_mir_snapshots() {
    for example in [
        "hello",
        "modules",
        "borrowed_shapes",
        "generic_aggregates",
        "stdlib_collections",
    ] {
        assert_example_mir_snapshot(example);
    }
}

fn assert_example_mir_snapshot(example: &str) {
    let project = example_fixture(example)
        .canonicalize()
        .unwrap_or_else(|err| panic!("canonicalize {example} fixture: {err}"));
    let mir = lower_project_to_mir(&project).expect("lower example MIR");
    let mut actual = serde_json::to_value(&mir).expect("serialize MIR snapshot");
    normalize_mir_snapshot_value(&mut actual, &project);
    let actual = serde_json::to_string_pretty(&actual).expect("render MIR snapshot") + "\n";

    let snapshot_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/mir_snapshots")
        .join(format!("{example}.json"));
    if std::env::var_os("AXIOM_UPDATE_MIR_SNAPSHOTS").is_some() {
        fs::create_dir_all(snapshot_path.parent().expect("snapshot dir"))
            .expect("create MIR snapshot dir");
        fs::write(&snapshot_path, actual).expect("update MIR snapshot");
        return;
    }

    let expected = fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", snapshot_path.display()));
    assert_eq!(
        actual, expected,
        "{example} normalized MIR snapshot drifted"
    );
}

fn example_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples")
        .join(name)
}

fn normalize_mir_snapshot_value(value: &mut Value, project: &Path) {
    match value {
        Value::String(text) => {
            let project = project.display().to_string();
            if text.starts_with(&project) {
                *text = text.replacen(&project, "<example>", 1);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_mir_snapshot_value(item, project);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                normalize_mir_snapshot_value(value, project);
            }
        }
        _ => {}
    }
}

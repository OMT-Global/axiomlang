use super::{CapabilityKind, stdlib_wrapper_capabilities};
use std::path::Path;

#[test]
fn stdlib_wrapper_capabilities_match_stdlib_paths_portably() {
    assert_eq!(
        stdlib_wrapper_capabilities(Path::new("<stdlib>/net.ax"), "resolve"),
        Some(vec![CapabilityKind::Net])
    );
    assert_eq!(
        stdlib_wrapper_capabilities(Path::new("<stdlib>\\net.ax"), "resolve"),
        Some(vec![CapabilityKind::Net])
    );
    assert_eq!(
        stdlib_wrapper_capabilities(Path::new("<stdlib>\\http_async.ax"), "async_serve_route"),
        Some(vec![CapabilityKind::Net, CapabilityKind::Async])
    );
}

#[test]
fn stdlib_catalog_effects_match_compiled_wrapper_capabilities() {
    let catalog: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../compiler-contracts/snapshots/stdlib-catalog.json"
    ))
    .expect("governed stdlib catalog JSON");
    assert_eq!(catalog["catalog_version"], "2.0.0");
    let modules = catalog["modules"].as_array().expect("catalog modules");
    assert_eq!(modules.len(), 34);
    let mut checked = 0;
    for module in modules {
        let file = module["name"]
            .as_str()
            .expect("module name")
            .strip_prefix("std/")
            .expect("stdlib module prefix");
        for symbol in module["symbols"].as_array().expect("module symbols") {
            let name = symbol["name"].as_str().expect("symbol name");
            for separator in ["/", "\\"] {
                let path = format!("<stdlib>{separator}{file}");
                let capabilities = stdlib_wrapper_capabilities(Path::new(&path), name)
                    .unwrap_or_default();
                let mut labels: Vec<String> = capabilities
                    .into_iter()
                    .map(|capability| {
                        serde_json::to_value(capability)
                            .expect("serializable capability")
                            .as_str()
                            .expect("capability label")
                            .to_owned()
                    })
                    .collect();
                labels.sort();
                labels.dedup();
                let effect = if labels.is_empty() {
                    "pure".to_owned()
                } else {
                    format!("capability:{}", labels.join(","))
                };
                assert_eq!(symbol["effect"].as_str(), Some(effect.as_str()), "{path}:{name}");
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 307);
    assert_eq!(stdlib_wrapper_capabilities(Path::new("src/net.ax"), "resolve"), None);
    assert_eq!(stdlib_wrapper_capabilities(Path::new("<stdlib>/nested/net.ax"), "resolve"), None);
}

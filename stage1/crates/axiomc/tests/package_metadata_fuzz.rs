//! Deterministic mutation coverage for the four untrusted metadata parsers.
use axiomc::package_trust::{
    MAX_DOCUMENT_BYTES, PackageTrustError, parse_package_signature_json, parse_registry_index_json,
    parse_trust_roots_json, parse_verification_expectation_json,
};
use serde_json::Value;

type Parser = fn(&[u8]) -> Result<Value, PackageTrustError>;

fn parsers() -> [(&'static str, Parser); 4] {
    [
        ("package_signature", |b| {
            parse_package_signature_json(b).map(|v| v.0)
        }),
        ("trust_roots", |b| parse_trust_roots_json(b).map(|v| v.0)),
        ("registry_index", |b| {
            parse_registry_index_json(b).map(|v| v.0)
        }),
        ("verification_expectation", |b| {
            parse_verification_expectation_json(b).map(|v| v.0)
        }),
    ]
}

fn contract() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../package-trust/contract/package-trust.json"
    ))
    .expect("checked-in metadata contract")
}

fn exercise(parser: Parser, bytes: &[u8], label: &str) -> bool {
    let result = std::panic::catch_unwind(|| parser(bytes))
        .unwrap_or_else(|_| panic!("metadata parser panicked: {label}; bytes={bytes:?}"));
    match result {
        Err(error) => {
            assert!(!error.to_string().is_empty(), "{label}: missing diagnostic");
            false
        }
        Ok(value) => {
            // Successful parsing must preserve the input JSON value and survive
            // serialization/reparse. Parsing is not cryptographic verification.
            assert_eq!(
                value,
                serde_json::from_slice::<Value>(bytes).unwrap(),
                "{label}"
            );
            let canonical = serde_json::to_vec(&value).unwrap();
            assert_eq!(
                parser(&canonical).unwrap(),
                value,
                "{label}: round-trip drift"
            );
            true
        }
    }
}

#[test]
fn bounded_metadata_mutations_preserve_parser_roundtrips_without_panics() {
    let contract = contract();
    for (name, parser) in parsers() {
        let seed = serde_json::to_vec(&contract[name]).unwrap();
        assert!(
            exercise(parser, &seed, name),
            "{name}: positive seed rejected"
        );
        // Sample at most 65 byte positions per document, including both ends.
        // The fixed index formula makes every case reproducible and independent
        // of random-number libraries or machine architecture.
        let mut positions: Vec<usize> = (0..=64).map(|n| n * seed.len() / 64).collect();
        positions.dedup();
        let mut accepted = 0;
        let mut rejected = 0;
        for offset in positions {
            let mut candidates = vec![seed[..offset].to_vec()];
            let mut inserted = seed.clone();
            inserted.insert(offset, 0xff);
            candidates.push(inserted);
            if offset < seed.len() {
                let mut flipped = seed.clone();
                flipped[offset] ^= 0x80;
                candidates.push(flipped);
                let mut deleted = seed.clone();
                deleted.remove(offset);
                candidates.push(deleted);
            }
            for (mutation, bytes) in candidates.iter().enumerate() {
                if exercise(
                    parser,
                    bytes,
                    &format!("{name}/offset-{offset}/mutation-{mutation}"),
                ) {
                    accepted += 1;
                } else {
                    rejected += 1;
                }
            }
        }
        assert!(
            rejected > 0,
            "{name}: mutation campaign never rejected input"
        );
        eprintln!("{name}: {accepted} accepted, {rejected} rejected mutations");
    }
}

#[test]
fn malformed_duplicate_and_over_budget_metadata_is_rejected() {
    let contract = contract();
    let deeply_nested = format!("{}null{}", "[".repeat(256), "]".repeat(256)).into_bytes();
    for (name, parser) in parsers() {
        let seed = serde_json::to_vec(&contract[name]).unwrap();
        for (label, prefix) in [
            ("duplicate", br#"{"contract":null,"#.as_slice()),
            ("escaped-duplicate", br#"{"\u0063ontract":null,"#.as_slice()),
        ] {
            let mut bytes = prefix.to_vec();
            bytes.extend_from_slice(&seed[1..]);
            let error = parser(&bytes).expect_err("duplicate members must be rejected");
            assert!(
                error.to_string().contains("duplicate JSON member"),
                "{name}/{label}: {error}"
            );
        }
        // Valid JSON plus whitespace isolates the size guard from syntax errors.
        let mut oversized = seed.clone();
        oversized.resize(MAX_DOCUMENT_BYTES + 1, b' ');
        let error = parser(&oversized).expect_err("document budget must be enforced");
        assert!(error.to_string().contains("work budget"), "{name}: {error}");
        for (label, bytes) in [
            ("depth", deeply_nested.as_slice()),
            ("invalid-utf8", b"{\"contract\":\"\xff\"}".as_slice()),
            (
                "truncated-string",
                b"{\"contract\":\"unterminated".as_slice(),
            ),
            ("trailing-value", b"{}{}".as_slice()),
            ("overflow-number", b"{\"n\":1e9999}".as_slice()),
            ("wrong-root", b"[]".as_slice()),
        ] {
            assert!(
                !exercise(parser, bytes, &format!("{name}/{label}")),
                "{name}/{label}: accepted invalid input"
            );
        }
    }
}

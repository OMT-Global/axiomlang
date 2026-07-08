.PHONY: test smoke supply-chain docs-python-exit docs-python-exit-test python-exit-readiness python-exit-readiness-github rust-exit-readiness rust-exit-readiness-github rust-exit-readiness-test rust-exit-command-surface-coverage rust-exit-command-surface-coverage-test self-hosting-language-readiness self-hosting-language-readiness-github self-hosting-language-readiness-test snapshot-bootstrap-readiness snapshot-bootstrap-readiness-test self-hosting-spike-parity stage1-compiler-source-monoliths stage1-compiler-source-monoliths-test stage1-full-lib-triage stage1-full-lib-triage-test stage1-direct-native-runtime-abi stage1-direct-native-runtime-abi-coverage stage1-direct-native-runtime-abi-evidence stage1-direct-native-runtime-abi-test stage1-direct-native-example-smoke stage1-direct-native-example-smoke-test stage1-axiom-dwarf-readiness-test stage1-package-graph-boundary stage1-package-graph-boundary-test stage1-diagnostics-syntax-boundary stage1-diagnostics-syntax-boundary-test stage1-command-lsp-boundary stage1-command-lsp-boundary-test stage1-hir-boundary stage1-hir-boundary-test stage1-mir-backend-boundary stage1-mir-backend-boundary-test stage1-test stage1-proof-test stage1-stdlib-test stage1-basic-smoke stage1-stdlib-smoke stage1-compiler-property-test stage1-conformance stage1-smoke stage1-bench stage1-bench-update-baseline stage1-bench-gate stage1-crap-proposal stage1-crap-thresholds stage1-crap-thresholds-test mutation-rust-smoke mutation-survivor-report stage1-run

test: docs-python-exit python-exit-readiness stage1-test

smoke: stage1-smoke

supply-chain:
	bash scripts/ci/run-toolchain-supply-chain.sh

docs-python-exit:
	bash scripts/ci/check-python-exit-docs.sh
	bash scripts/ci/test-check-python-exit-docs.sh
	bash scripts/ci/test-check-python-exit-readiness.sh

docs-python-exit-test:
	bash scripts/ci/test-check-python-exit-docs.sh

python-exit-readiness:
	bash scripts/ci/check-python-exit-readiness.sh --json

python-exit-readiness-github:
	bash scripts/ci/check-python-exit-readiness.sh --json --require-issue-states

rust-exit-readiness:
	bash scripts/ci/check-rust-exit-readiness.sh --json

rust-exit-readiness-github:
	bash scripts/ci/check-rust-exit-readiness.sh --json --require-issue-states

rust-exit-readiness-test:
	bash scripts/ci/test-check-rust-exit-readiness.sh
	bash scripts/ci/test-check-rust-exit-command-surface.sh

rust-exit-command-surface-coverage:
	python3 scripts/ci/check-rust-exit-command-surface.py --json

rust-exit-command-surface-coverage-test:
	bash scripts/ci/test-check-rust-exit-command-surface.sh

self-hosting-language-readiness:
	python3 scripts/ci/check-self-hosting-language-readiness.py --json

self-hosting-language-readiness-github:
	python3 scripts/ci/check-self-hosting-language-readiness.py --json --require-issue-states

self-hosting-language-readiness-test:
	bash scripts/ci/test-check-self-hosting-language-readiness.sh

snapshot-bootstrap-readiness:
	python3 scripts/ci/check-snapshot-bootstrap-readiness.py --json

snapshot-bootstrap-readiness-test:
	bash scripts/ci/test-check-snapshot-bootstrap-readiness.sh

self-hosting-spike-parity:
	bash scripts/ci/run-self-hosting-spike-parity.sh

stage1-compiler-source-monoliths:
	python3 scripts/ci/report-compiler-source-monoliths.py --json --check-plan --check-ratchet

stage1-compiler-source-monoliths-test:
	python3 scripts/ci/test-report-compiler-source-monoliths.py

stage1-full-lib-triage:
	python3 scripts/ci/check-stage1-full-lib-triage.py --json

stage1-full-lib-triage-test:
	bash scripts/ci/test-check-stage1-full-lib-triage.sh

stage1-direct-native-runtime-abi:
	python3 scripts/ci/check-direct-native-runtime-abi.py --json

stage1-direct-native-runtime-abi-coverage:
	python3 scripts/ci/check-direct-native-runtime-abi.py --coverage-matrix --json

stage1-direct-native-runtime-abi-evidence:
	bash scripts/ci/run-direct-native-runtime-abi-evidence.sh

stage1-direct-native-runtime-abi-test:
	bash scripts/ci/test-check-direct-native-runtime-abi.sh
	bash scripts/ci/test-run-direct-native-runtime-abi-evidence.sh
	bash scripts/ci/test-run-direct-native-example-smoke.sh
	bash scripts/ci/test-render-direct-native-runtime-abi-status.sh

stage1-direct-native-example-smoke:
	bash scripts/ci/run-direct-native-example-smoke.sh

stage1-direct-native-example-smoke-test:
	bash scripts/ci/test-run-direct-native-example-smoke.sh

stage1-axiom-dwarf-readiness-test:
	python3 -m py_compile scripts/debug/check-axiom-dwarf.py scripts/debug/test-check-axiom-dwarf.py
	python3 scripts/debug/test-check-axiom-dwarf.py

stage1-package-graph-boundary:
	python3 scripts/ci/check-package-graph-boundary.py --json

stage1-package-graph-boundary-test:
	bash scripts/ci/test-check-package-graph-boundary.sh

stage1-diagnostics-syntax-boundary:
	python3 scripts/ci/check-diagnostics-syntax-boundary.py --json

stage1-diagnostics-syntax-boundary-test:
	bash scripts/ci/test-check-diagnostics-syntax-boundary.sh

stage1-command-lsp-boundary:
	python3 scripts/ci/check-command-lsp-boundary.py --json

stage1-command-lsp-boundary-test:
	bash scripts/ci/test-check-command-lsp-boundary.sh

stage1-hir-boundary:
	python3 scripts/ci/check-hir-boundary.py --json

stage1-hir-boundary-test:
	bash scripts/ci/test-check-hir-boundary.sh

stage1-mir-backend-boundary:
	python3 scripts/ci/check-mir-backend-boundary.py --json

stage1-mir-backend-boundary-test:
	bash scripts/ci/test-check-mir-backend-boundary.sh

stage1-test:
	RUST_MIN_STACK=8388608 cargo test --manifest-path stage1/Cargo.toml --features run-native-tests
	$(MAKE) stage1-stdlib-test
	$(MAKE) stage1-compiler-property-test
	$(MAKE) stage1-proof-test

stage1-proof-test:
	bash scripts/ci/run-stage1-proof-test.sh

stage1-stdlib-test:
	bash scripts/ci/run-stdlib-property-checks.sh

stage1-compiler-property-test:
	bash scripts/ci/run-compiler-property-checks.sh

stage1-conformance:
	bash scripts/ci/run-stage1-conformance.sh

stage1-bench:
	python3 scripts/ci/run-stage1-bench.py --output stage1/benchmarks/generated/stage1-bench.json

stage1-bench-update-baseline:
	python3 scripts/ci/run-stage1-bench.py --output stage1/benchmarks/stage1-baseline.json

stage1-bench-gate:
	python3 scripts/ci/check-stage1-benchmarks.py
	python3 scripts/ci/report-stage1-reference-comparison.py

mutation-survivor-report:
	python3 scripts/ci/render-mutation-survivor-report.py \
		--input .axiom-build/reports/mutation-rust-smoke.json \
		--output .axiom-build/reports/mutation-survivors.md

stage1-crap-proposal:
	python3 scripts/ci/propose-stage1-crap-thresholds.py --output stage1/quality/crap-threshold-proposal.json

mutation-rust-smoke:
	python3 scripts/ci/run-mutation-rust-smoke.py

stage1-crap-thresholds:
	python3 scripts/ci/propose-stage1-crap-thresholds.py

stage1-crap-thresholds-test:
	bash scripts/ci/test-propose-stage1-crap-thresholds.sh

stage1-basic-smoke:
	bash scripts/ci/run-stage1-basic-smoke.sh

stage1-stdlib-smoke:
	bash scripts/ci/run-stage1-stdlib-smoke.sh

stage1-smoke:
	$(MAKE) stage1-basic-smoke
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/capabilities --json
	$(MAKE) stage1-stdlib-smoke
	$(MAKE) stage1-proof-test
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- caps stage1/examples/hello --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt stage1/examples/hello --check
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc stage1/examples/hello --out-dir .axiom-build/docs/hello
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc --md stage1/examples/hello --out-dir .axiom-build/docs/hello-md
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json

stage1-run:
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello

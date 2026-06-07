.PHONY: test smoke supply-chain docs-python-exit docs-python-exit-test python-exit-readiness python-exit-readiness-github rust-exit-readiness rust-exit-readiness-github rust-exit-readiness-test stage1-direct-native-runtime-abi stage1-direct-native-runtime-abi-test stage1-package-graph-boundary stage1-package-graph-boundary-test stage1-diagnostics-syntax-boundary stage1-diagnostics-syntax-boundary-test stage1-test stage1-proof-test stage1-stdlib-test stage1-compiler-property-test stage1-conformance stage1-smoke stage1-bench stage1-bench-update-baseline stage1-bench-gate stage1-crap-proposal stage1-crap-thresholds stage1-crap-thresholds-test mutation-rust-smoke mutation-survivor-report stage1-run

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

stage1-direct-native-runtime-abi:
	python3 scripts/ci/check-direct-native-runtime-abi.py --json

stage1-direct-native-runtime-abi-test:
	bash scripts/ci/test-check-direct-native-runtime-abi.sh

stage1-package-graph-boundary:
	python3 scripts/ci/check-package-graph-boundary.py --json

stage1-package-graph-boundary-test:
	bash scripts/ci/test-check-package-graph-boundary.sh

stage1-diagnostics-syntax-boundary:
	python3 scripts/ci/check-diagnostics-syntax-boundary.py --json

stage1-diagnostics-syntax-boundary-test:
	bash scripts/ci/test-check-diagnostics-syntax-boundary.sh

stage1-test:
	RUST_MIN_STACK=8388608 cargo test --manifest-path stage1/Cargo.toml --features run-native-tests
	$(MAKE) stage1-stdlib-test
	$(MAKE) stage1-compiler-property-test
	$(MAKE) stage1-proof-test

stage1-proof-test:
	for example in proof_cli proof_worker proof_http_service; do \
		cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/$$example --json; \
		cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/$$example --json; \
		cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/$$example; \
		cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/$$example --json; \
	done

stage1-stdlib-test:
	bash scripts/ci/run-stdlib-property-checks.sh

stage1-compiler-property-test:
	bash scripts/ci/run-compiler-property-checks.sh

stage1-conformance:
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test --conformance --json

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

stage1-smoke:
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/hello --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/hello --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/modules --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/modules --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/modules
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/modules --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/packages --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/packages --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/packages
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/packages --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/workspace --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/workspace --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/workspace
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/workspace --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/workspace_only --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/workspace_only --package workspace-app --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/workspace_only --package workspace-app
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/workspace_only --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/capabilities --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/capabilities --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/capabilities
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/capabilities --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/arrays --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/arrays --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/arrays
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/slices --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/slices --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/slices
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/borrowed_shapes --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/borrowed_shapes --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/borrowed_shapes
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/tuples --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/tuples --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/tuples
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/maps --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/maps --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/maps
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/structs --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/structs --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/structs
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/enums --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/enums --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/enums
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/outcomes --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/outcomes --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/outcomes
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/generic_aggregates --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/generic_aggregates --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/generic_aggregates
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_time --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_time --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_time
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_env --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_env --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_env
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_fs --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_fs --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_fs
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_net --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_net --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_net
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_process --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_process --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_process
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_crypto_hash --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_crypto_hash --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_crypto_hash
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_io --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_io --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_io
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_json --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_json --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_json

	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_regex --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_regex --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_regex
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_regex --json

	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_testing --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_testing --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_testing
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_testing --include-benchmarks --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_collections --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_collections --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_collections
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_collections --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_string_builder --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_string_builder --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_string_builder
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_string_builder --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_log --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_log --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_log
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_log --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_sync --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_sync --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_sync
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_sync --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_async --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_async --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_async
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/stdlib_async --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/stdlib_http --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/stdlib_http --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/stdlib_http
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/proof_cli --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/proof_cli --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/proof_cli
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/proof_cli --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/proof_worker --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/proof_worker --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/proof_worker
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/proof_worker --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- check stage1/examples/proof_http_service --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- build stage1/examples/proof_http_service --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/proof_http_service
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- test stage1/examples/proof_http_service --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- caps stage1/examples/hello --json
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- fmt stage1/examples/hello --check
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc stage1/examples/hello --out-dir .axiom-build/docs/hello
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- doc --md stage1/examples/hello --out-dir .axiom-build/docs/hello-md
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- bench stage1/examples/benchmarks --json

stage1-run:
	cargo run --manifest-path stage1/Cargo.toml -p axiomc -- run stage1/examples/hello

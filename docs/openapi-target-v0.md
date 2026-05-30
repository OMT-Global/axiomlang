# OpenAPI Target v0

OpenAPI Target v0 projects Axiom HTTP-serving intent into an OpenAPI 3.1 JSON
document. It is an artifact target: it does not change runtime serving behavior
or capability enforcement.

## Target Contract

```json
{
  "id": "axiom://target/stage1-openapi-v0",
  "class": "openapi_spec",
  "description": "Stage 1 OpenAPI generator for HTTP-serving semantic routes.",
  "status": "experimental",
  "input_node_kinds": ["Package", "Module", "Function", "Capability", "Effect", "Type"],
  "supported_effect_kinds": ["network.http.get", "network.tcp.bind"],
  "supported_type_features": ["aggregate.struct", "aggregate.enum"],
  "artifact_outputs": [
    {
      "id": "axiom://package/<package>/artifact/openapi-spec",
      "kind": "openapi_spec",
      "path": "dist/openapi.json",
      "generated_from": ["axiom://package/<package>"],
      "status": "planned"
    }
  ],
  "evidence_requirements": ["unit_test", "fixture"],
  "unsupported_feature_diagnostics": []
}
```

## Command

```bash
axiomc generate openapi <path> --out dist/openapi.json --json
```

Relative `--out` paths are resolved inside the package path. The generator
emits a JSON report with the target contract, generated artifact record,
discovered routes, and non-fatal diagnostics.

## Projection Rules

The generator consumes the existing `std/http.ax` route surface:

- `route(path, body)` produces a `GET` operation with a `200` plain-text
  string response.
- `route_response(path, response(...))` and `HttpResponse` literals preserve
  literal status codes and literal `content-type` headers.
- `serve(bind, selected_route, max_requests)` marks a discovered route as
  served when the route is passed directly or through a local `let` binding.
- Direct `http_serve_route(bind, path, body, max_requests)` calls are accepted
  as the lowered intrinsic form.

Only literal route paths are projected in v0. Dynamic paths are skipped instead
of being guessed. A package with no discoverable HTTP-serving routes still
emits a valid OpenAPI document with an empty `paths` object and a diagnostic in
the command report.

## Artifact Plan

`axiomc inspect artifacts <path> --json` reports `dist/openapi.json` as an
`openapi_spec` target artifact. Its status is `planned` before generation and
`generated` once the file exists.

## Boundaries

OpenAPI Target v0 is contract generation only. It does not add AsyncAPI, gRPC,
reverse OpenAPI import, runtime routing, or new capability gates.

# SQL Migration Target v0

SQL Migration Target v0 projects declared package data shapes into a
deterministic PostgreSQL-compatible migration pair. It is an artifact target:
it does not connect to a database or execute migrations.

## Declaration Surface

The v0 declaration surface reuses stage1 public structs. Public structs in
schema files are treated as persistent tables:

- `src/schema.ax`
- files under `src/schema/`
- `schema.ax`
- files under `schema/`

Only scalar fields are projected in v0. `int`, numeric widths, `bool`,
`string`, and `Option<T>` over those scalar types are supported. A non-null
`id: int` field is treated as the primary key. Data invariants remain declared
as `axiom` nodes and are carried in the target contract as semantic inputs, but
v0 does not synthesize SQL `CHECK` constraints from free-form axiom text.

## Target Contract

```json
{
  "id": "axiom://target/stage1-sql-migration-v0",
  "class": "sql_migration",
  "description": "Stage 1 SQL migration generator for declared schema structs and invariants.",
  "status": "experimental",
  "input_node_kinds": ["Package", "Module", "Type", "Axiom"],
  "supported_effect_kinds": [],
  "supported_type_features": [
    "numeric.signed",
    "numeric.unsigned",
    "numeric.float",
    "aggregate.struct"
  ],
  "artifact_outputs": [
    {
      "id": "axiom://package/<package>/artifact/sql-migration/001-schema-forward-sql",
      "kind": "sql_migration",
      "path": "dist/sql/001_schema_forward.sql",
      "generated_from": ["axiom://package/<package>"],
      "status": "generated"
    }
  ],
  "evidence_requirements": ["unit_test", "fixture"],
  "unsupported_feature_diagnostics": []
}
```

## Command

```bash
axiomc generate sql <path> --out dist/sql --json
```

The command writes:

- `001_schema_forward.sql`
- `001_schema_rollback.sql`
- `schema.snapshot.json`

Relative `--out` paths are resolved inside the package path. The JSON report
includes the target contract, generated artifact records, previous schema,
current schema, and a byte-change flag. If any schema field uses an unsupported
type, or if a projected table does not declare a non-null `id: int` primary key,
the command fails before writing migration artifacts.

## Diff Model

The generator compares the current declared schema against the first available
baseline:

1. `<package>/schema.previous.json`
2. `<out>/schema.snapshot.json`
3. an empty schema

Added tables emit `CREATE TABLE`; removed tables emit `DROP TABLE IF EXISTS`.
Added and removed columns emit `ALTER TABLE` statements. Type and nullability
changes emit deterministic `ALTER COLUMN` statements. The rollback file is the
inverse diff.

Re-running with the same declared schema does not rewrite identical output
bytes and reports `"changed": false`.

## Boundaries

SQL Migration Target v0 does not execute migrations, connect to a live
database, generate ORM/query code, or claim a dialect matrix. PostgreSQL is the
initial SQL rendering target.

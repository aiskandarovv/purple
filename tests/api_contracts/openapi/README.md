# OpenAPI Schema Fragments

Minimal response-model extracts from provider OpenAPI specs. Each file defines
the required fields for one API endpoint response, used by
`tests/schema_validation.rs` to verify our golden fixtures stay in sync with
upstream specs.

## Format

- `_meta.provider`: provider name
- `_meta.endpoint`: HTTP method + path
- `_meta.spec_url`: URL where the spec was downloaded
- `_meta.spec_version`: API version or spec commit/date
- `_meta.last_verified`: date this fragment was last checked against the live spec
- `fixture`: filename of the golden fixture this validates (in parent dir)
- `required_paths`: array of dot-notation paths that the spec says MUST exist

Paths use the same syntax as `assert_has_key`: dots for nesting, `[0]` for
array indexing (checks first element).

## Maintenance

When a provider updates their API spec:
1. Re-download the relevant response model
2. Update `required_paths` to match new required fields
3. Update the golden fixture if it is missing new required fields
4. Update `_meta.last_verified`

## Providers without OpenAPI specs

AWS (XML, no OpenAPI), Proxmox (no published spec), i3D.net (no published spec)
and TransIP (no published spec) are validated by contract snapshot tests only.

# Tools

## Static registry

A Tool is immutable metadata: name, description and JSON Schema text.
ToolSet validates basic descriptor constraints and duplicate names without
allocating. It preserves declaration order and performs exact-name lookup.

The current registry is a borrowed slice and uses linear lookup. This is
appropriate for small registries and avoids an allocation or a mandatory hash
index. Large-registry alternatives require benchmark evidence before becoming
part of the public API.

## Execution boundary

Executor receives a name, complete JSON argument bytes and caller-owned
output storage. It does not grant permissions or decide whether a tool call is
safe. The application must authorize each invocation before calling its
executor.

The current descriptor check is not a JSON Schema validator. Schema validation,
business validation, authorization and approval are separate application
responsibilities.

# Tools

ToolSet borrows immutable descriptors. Construction validates schema JSON and
unique names; it does not enforce the full JSON Schema vocabulary. ToolSet::new
uses a fixed 128-token temporary once. with_storage accepts caller-sized storage.
Names can include namespace separators, such as fs.read and repo:search.

Linear get has no auxiliary memory. indexed fills a caller-provided usize slice,
sorts indexes without changing descriptor order and enables binary lookup. Choose
based on measured tool counts; no universal crossover point is asserted.
FixedRegistry uses caller-owned Option<Tool> slots and rejects duplicates or
capacity exhaustion without changing accepted entries.

Executor receives Arguments, not unvalidated bytes. Application schema validation,
authorization, cost limits and dangerous-operation approval remain mandatory
application responsibilities. ValidationPolicy is an optional generic hook, not
an implicit permission grant.

A tool can return ToolOutput::failure(n) with a UTF-8 explanation in caller output.
Protocol/input failures return Error. An executor returning an out-of-bounds
length or invalid UTF-8 is rejected. Never replay side effects automatically after
an output capacity or serialization error.

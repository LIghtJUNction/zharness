# Security

zharness is a low-level composition library. It is not a security boundary by
itself.

- Tool calls from a model are untrusted input.
- A complete JSON object is not proof that its fields satisfy an application's
  schema or business rules.
- The application must authorize every tool invocation and decide whether
  human approval is required.
- Executors must enforce their own resource, filesystem, network and time
  limits.
- Caller-provided buffers must be sized for explicit application limits.
- Capacity errors must be handled; callers must not retry with unbounded growth.
- Transport authentication, origin checks, HTTP header validation and process
  isolation belong to the transport/application layer.
- Do not log credentials, private tool arguments or sensitive tool results.

The parser's depth bound limits recursive JSON nesting. It does not replace
message-size limits or application-level rate limiting.

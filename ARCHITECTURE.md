# Architecture

zharness is a Sans-I/O composition layer. It owns neither the agent loop nor
application policy.

## Dependency direction

zharness -> zcall -> zconv

- zconv owns provider wire formats, parsing, conversion and its intermediate
  representation.
- zcall owns protocol-independent model requests, normalized stream events,
  tool readiness and model-turn lifecycle.
- zharness owns reusable harness-side boundaries: MCP messages, tool metadata,
  bounded dispatch and adapters between public zcall events and user executors.
- The application owns transport, credentials, retries, persistence, model
  selection, authorization, scheduling and the agent loop.

## Memory model

Input, output, scratch and long-lived workspaces belong to the caller. Operations
must either complete within those regions or return a capacity error. No global
mutable state or implicit runtime is required.

## Composition

MCP and tool dispatch are optional, independent modules. An application may
use the registry without MCP, or expose an MCP endpoint without using zcall.
std is a convenience boundary, not a requirement of the protocol core.

## Current limitations

The initial repository is an early implementation, not yet the complete
architecture described above. See docs/MCP.md. The zcall bridge currently
exposes a borrowed invocation view; complete tool-result submission and
RequestBuilder integration remain to be completed and tested.

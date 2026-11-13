# Security and ownership

Model output, tool arguments, metadata, summaries, task payloads and agent messages
are untrusted. Registered names, valid JSON, pinned context and capability bits
are not authorization decisions. The application validates schema/business rules
and approves dangerous operations at the actual execution boundary.

The library core is no_std and forbids handwritten unsafe code. It uses caller-owned fixed buffers
and borrowed data, not hidden growing caches. Configure token, byte, object-field,
string, depth, group, tool-call, round and task limits for the application. Core
capacity failures do not automatically allocate or retry effects. Generic hooks,
executors and payload types can independently allocate; audit those separately.

The optional std skills adapter allocates bounded discovery state and reads local
files only when called. Configure trusted roots and inspect discovery diagnostics.
SKILLS_PATH can override project/global definitions, and discovered symlink roots
can point outside their lexical parent. Resource reads reject absolute paths,
parent traversal and resolved symlink escape from the selected skill directory.
These path checks assume a trusted local filesystem; they do not isolate hostile
concurrent mutations. Skill instructions and optional frontmatter such as
allowed-tools never grant execution permissions by themselves.

Native plugin loading is an explicit unsafe operation through zplugin's loader.
Library initializers execute before interface binding, so trust applies to the
entire native image and its dependencies. zplugin's generated FFI and runtime
implement the unsafe boundary; checked byte lengths do not sandbox native code.
Proxies retain their library borrow, remain on their creating thread and use
mutable access for calls. The default loader policy pins code until process exit.
Cooperative unloading additionally requires the plugin's UnloadSafe contract.
Plugin errors retain their original status; discard partial output after errors
and never automatically retry side effects to obtain a larger buffer. The
Executor bridge preserves the caller's tool registry and hook decisions; native
code itself runs with the host process's privileges.

The explicit wasm feature accepts untrusted binary modules through zplugin's
validated, import-free Wasm32 protocol. Guest code receives checked byte buffers,
not host pointers or capabilities. Memory/table/stack/fuel limits are mandatory;
traps poison the instance, and unknown operation names do not enter it. Detailed
engine diagnostics remain in the adapter while normal harness errors stay Copy.
Use Prepared execution hooks to authorize routed public names; direct adapter
calls deliberately remain low-level application operations.

Wasm isolation depends on the engine and does not impose a process-wide RSS cap
or compilation wall-clock deadline. Bound source reads, compilation rate and the
number of instances, and add OS process isolation when required by the threat
model. Do not reuse a stateful plugin across confidential tenants: it can retain
prior inputs. Sandbox loading grants no WASI/network/filesystem/clock/host calls.
Native async tasks, callbacks and resources are not automatically bridged to Wasm.
Replacing an instance validates its module/interface first; initialization and
state migration at the business level remain explicit host decisions.

Context token counts are supplied by the application and may be estimates. Reserve
for output and request overhead, and recompute for the target model. An incorrect
cost cannot be turned into a hard model-context guarantee. Keep call/result groups
whole during eviction. Pin trusted instructions, and do not elevate untrusted
summaries into trusted instructions without an application decision.

ToolReady is accepted only after zcall completes argument validation. The native
loop additionally waits for WaitingTools before scheduling work. A paused stream
event can replay: return Continue only after retention succeeds, and never execute
a side effect then return Pause. Tests exercise buffer growth and replay against
a real zcall Session. Late/duplicate results are rejected by dispatch tickets and
real call-ID checks. Keep history and output buffers alive until consumers finish.

Before hooks may run repeatedly and must not execute the proposed work. After
observers see committed state and cannot roll it back. Hooks are not a sandbox;
the application must not allow callback panic or blocking work to break its own
consistency or latency contract. No implicit permission grant accompanies NoHooks.

Cancellation signals state, not external rollback. Lease expiry, handoff and
renewal fence only coordinator completions. Use monotonic coordinator time, fresh
run IDs/board epochs and external idempotency/fencing before retrying real effects.
Neither task payloads nor mailbox sender IDs authenticate a remote peer. Networking,
encryption, access control, durable recovery and distributed agreement are outside
this crate. Mailbox and TaskBoard access must be serialized by the application.

Never re-execute a side-effecting tool merely to enlarge a result-encoding buffer.
Retain its result and retry only the failed serialization. Clear operations release
logical state; they do not securely erase every byte of retained storage.

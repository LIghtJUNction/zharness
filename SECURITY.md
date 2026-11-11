# Security

Never treat model output, syntactically valid JSON or a registered tool name as
an authorization decision. Applications decide which tools may run and perform
schema and business validation before side effects.

ArgumentLimits caps JSON depth, input length, token count, object fields and decoded
string size. Integer policy rejects out-of-range integer lexemes by default;
turning it off preserves the exact original bytes rather than rounding. Error
variants preserve scalar offsets and the underlying zcall error metadata.

Prepare never executes a tool. InvocationStorage capacity failures preserve the
previous call. Executor output bounds and UTF-8 are checked before exposing a
Completion. Output-buffer exhaustion must not cause automatic retries of work
that may already have side effects. Retain business failures as is_error results.

ToolReady borrowing ends with the callback unless the application copies data.
Use one caller-owned retention slot per concurrent call. Do not return Pause
after executing an operation: zcall may replay that event. The provided example
performs retention only in callbacks and executes after stream completion.

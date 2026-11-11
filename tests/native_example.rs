#[path = "../examples/tool_loop.rs"]
mod tool_loop;

#[test]
fn controller_context_hooks_and_zcall_complete_two_turns() {
    tool_loop::main();
}

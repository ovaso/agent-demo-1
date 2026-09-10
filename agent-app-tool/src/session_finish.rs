use agent_core::tool;

#[tool(
    created_at = 1788784117,
    version = "v1.0.0-20260910",
    finish_session,
    description = "当用户明确表示要结束、退出、退下或今天到此为止时调用。summary 必须简洁概括本次会话的重要结论。"
)]
fn session_finish(summary: String) -> String {
    summary
}

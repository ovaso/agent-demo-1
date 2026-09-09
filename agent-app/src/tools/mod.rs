use agent_core::tool;

mod run_cmd;
mod write_file;

pub(super) use run_cmd::RunCmd;
pub(super) use write_file::WriteFile;

#[tool]
pub(super) fn echo(text: String) -> String {
    text
}

#[tool(
    finish_session,
    description = "当用户明确表示要结束、退出、退下或今天到此为止时调用。summary 必须简洁概括本次会话的重要结论。"
)]
pub(super) fn session_finish(summary: String) -> String {
    summary
}

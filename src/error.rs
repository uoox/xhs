use serde_json::{json, Value};
use std::fmt;

#[derive(Debug)]
pub struct XhsError {
    pub code: &'static str,
    pub message: String,
    pub hint: String,
    pub api_code: Option<i64>,
}

pub type Result<T> = std::result::Result<T, XhsError>;

impl XhsError {
    pub fn new(code: &'static str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        XhsError {
            code,
            message: message.into(),
            hint: hint.into(),
            api_code: None,
        }
    }
    pub fn arg(message: impl Into<String>) -> Self {
        Self::new("invalid_arg", message, "看 `xhs <命令> --help` 的参数说明")
    }
    pub fn confirm(action: &str) -> Self {
        Self::new(
            "confirm_required",
            format!("{action} 是写操作，没有加 --yes，未执行"),
            "确认无误后在命令末尾加 --yes（或 --execute）再执行一次",
        )
    }
    pub fn io(message: impl Into<String>) -> Self {
        Self::new("io_error", message, "检查路径权限和磁盘空间")
    }
    pub fn exit_code(&self) -> i32 {
        match self.code {
            "invalid_arg" | "confirm_required" | "need_xsec_token" => 2,
            "not_logged_in" => 3,
            "risk_stop" | "locked" => 4,
            "rate_limited" => 5,
            "not_found" => 6,
            "amcu_missing" | "browser_unavailable" | "ambiguous_profile" => 7,
            _ => 1,
        }
    }
    pub fn to_json(&self) -> Value {
        let mut e = json!({"code": self.code, "message": self.message, "hint": self.hint});
        if let Some(c) = self.api_code {
            e["api_code"] = json!(c);
        }
        json!({"ok": false, "error": e})
    }
}

impl fmt::Display for XhsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "错误 [{}]: {}", self.code, self.message)?;
        if !self.hint.is_empty() {
            write!(f, "\n  下一步: {}", self.hint)?;
        }
        Ok(())
    }
}

pub const HARD_RISK: &[i64] = &[300011, 300012, 300015, 461];
pub const SOFT_RISK: &[i64] = &[300013, 300017, 300031, 471];

pub fn risk_hint(profile: &str) -> String {
    format!("不要再试，账号可能已被踢。去 Chrome（profile {profile}）打开 xiaohongshu.com 重新登录，确认能正常浏览后执行 `xhs unlock --profile {profile}` 解锁")
}

pub fn classify_api(profile: &str, code: Option<i64>, status: Option<i64>, msg: &str, what: &str) -> XhsError {
    let c = code.or(status.filter(|s| *s == 461 || *s == 471));
    let mut e = match c {
        Some(c) if HARD_RISK.contains(&c) || status == Some(461) => {
            XhsError::new("risk_stop", format!("{what} 触发风控 {c} {msg}，已停止并锁定该 profile"), risk_hint(profile))
        }
        Some(c) if SOFT_RISK.contains(&c) => XhsError::new(
            "rate_limited",
            format!("{what} 被限流 {c} {msg}"),
            "冷却 10 分钟以上再试，减少 --pages，别连续跑；多次出现就停一天",
        ),
        Some(-100) | Some(-101) | Some(-104) => XhsError::new(
            "not_logged_in",
            format!("{what} 失败：登录态失效 {} {msg}", c.unwrap()),
            format!("在 Chrome（profile {profile}）里打开 xiaohongshu.com 登录后重试"),
        ),
        Some(-510001) | Some(-510000) | Some(300030) => XhsError::new(
            "not_found",
            format!("{what} 失败：内容不存在或已删除 {} {msg}", c.unwrap()),
            "确认 id 是否正确、笔记是否已删除或设为私密",
        ),
        _ => XhsError::new(
            "api_error",
            format!("{what} 失败：{} {msg}", c.map(|x| x.to_string()).unwrap_or_default()),
            "看 message 判断原因；参数错误先查 --help，偶发错误隔几分钟再试",
        ),
    };
    e.api_code = c;
    e
}

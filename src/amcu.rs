use crate::error::{Result, XhsError};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Amcu {
    pub bin: PathBuf,
    pub profile: Option<String>,
    pub session: String,
}

pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Amcu {
    pub fn new(profile: Option<String>, session: String) -> Amcu {
        let bin = std::env::var("XHS_AMCU").map(PathBuf::from).unwrap_or_else(|_| {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/bin/amcu");
            if home.exists() {
                home
            } else {
                PathBuf::from("amcu")
            }
        });
        Amcu { bin, profile, session }
    }

    pub fn exec(&self, args: &[String], timeout: Duration) -> Result<Out> {
        let mut cmd = Command::new(&self.bin);
        cmd.arg("browser");
        if !args.first().map(|a| a == "profiles" || a == "doctor").unwrap_or(false) {
            cmd.arg("--session").arg(&self.session);
        }
        if let Some(p) = &self.profile {
            if p.contains(':') && p.rsplit(':').next().map(|x| x.chars().all(|c| c.is_ascii_digit())).unwrap_or(false) {
                cmd.arg("--browser").arg(p);
            } else {
                cmd.arg("--profile").arg(p);
            }
        }
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| {
            XhsError::new(
                "amcu_missing",
                format!("启动 amcu 失败（{}）: {e}", self.bin.display()),
                "安装 amcu 到 ~/.local/bin/amcu，或用环境变量 XHS_AMCU 指定路径；装好后 `amcu browser doctor` 检查",
            )
        })?;
        let mut so = child.stdout.take().unwrap();
        let mut se = child.stderr.take().unwrap();
        let t1 = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = so.read_to_end(&mut b);
            b
        });
        let t2 = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = se.read_to_end(&mut b);
            b
        });
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if start.elapsed() > timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(40)),
                Err(_) => break None,
            }
        };
        let stdout = String::from_utf8_lossy(&t1.join().unwrap_or_default()).into_owned();
        let stderr = String::from_utf8_lossy(&t2.join().unwrap_or_default()).into_owned();
        match status {
            None => Err(XhsError::new(
                "timeout",
                format!("amcu {} 超过 {}s 没返回", args.first().cloned().unwrap_or_default(), timeout.as_secs()),
                "Chrome 可能卡住或标签页在加载；`amcu browser doctor` 检查后重试",
            )),
            Some(s) => Ok(Out {
                ok: s.success(),
                stdout,
                stderr,
            }),
        }
    }

    pub fn run(&self, args: &[String], timeout: Duration) -> Result<String> {
        let out = self.exec(args, timeout)?;
        if out.ok {
            return Ok(out.stdout);
        }
        Err(self.map_err(&format!("{}{}", out.stderr, out.stdout)))
    }

    pub fn map_err(&self, text: &str) -> XhsError {
        let t = text.trim();
        let code = t
            .split_once("error [")
            .and_then(|(_, r)| r.split_once(']'))
            .map(|(c, _)| c.to_string())
            .unwrap_or_default();
        let first = t.lines().next().unwrap_or("").to_string();
        let prof = self.profile.clone().unwrap_or_default();
        match code.as_str() {
            "ambiguous_browser" => XhsError::new(
                "ambiguous_profile",
                "连着多个 Chrome profile，没指定用哪个",
                "加 --profile <label>（`xhs profiles` 列出可选值），或设环境变量 XHS_PROFILE",
            ),
            "no_browser" | "not_connected" | "bridge_unavailable" | "no_connection" | "profile_not_found" | "unknown_profile" => XhsError::new(
                "browser_unavailable",
                format!(
                    "amcu 连不上浏览器{}：{first}",
                    if prof.is_empty() { String::new() } else { format!("（profile {prof}）") }
                ),
                "确认 Chrome 开着且该 profile 装了 amcu 扩展：`amcu browser doctor`、`xhs profiles`",
            ),
            "tab_not_found" => XhsError::new("tab_gone", first, "标签页没了，重试会自动重开"),
            _ => XhsError::new(
                "amcu_error",
                if t.is_empty() {
                    "amcu 返回失败但没有输出".to_string()
                } else {
                    t.chars().take(400).collect()
                },
                "看 amcu 给的 next 提示；`amcu browser doctor` 检查连接",
            ),
        }
    }
}

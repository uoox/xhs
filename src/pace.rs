use crate::error::{risk_hint, Result, XhsError};
use crate::util::{now_ms, rand_range, read_json, sleep_ms, state_dir, write_json, FileLock};
use serde_json::json;
use std::path::PathBuf;

pub fn safe_key(profile: &str) -> String {
    let k: String = profile
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if k.is_empty() {
        "default".into()
    } else {
        k
    }
}

fn kicked_path(key: &str) -> PathBuf {
    state_dir().join(format!("kicked-{}.json", safe_key(key)))
}

pub fn check_kicked(profile: &str) -> Result<()> {
    let p = kicked_path(profile);
    if !p.exists() {
        return Ok(());
    }
    let v = read_json(&p);
    Err(XhsError::new(
        "locked",
        format!(
            "profile {} 在 {} 触发过风控 {}（{}），已锁定，所有请求都不会发出",
            if profile.is_empty() { "default" } else { profile },
            v["time"].as_str().unwrap_or("?"),
            v["code"],
            v["message"].as_str().unwrap_or("")
        ),
        risk_hint(if profile.is_empty() { "<label>" } else { profile }),
    ))
}

pub fn set_kicked(profile: &str, code: Option<i64>, message: &str) {
    let t = chrono::Utc::now()
        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let _ = write_json(&kicked_path(profile), &json!({"profile": profile, "code": code, "message": message, "time": t}));
}

pub fn unlock(profile: &str) -> bool {
    std::fs::remove_file(kicked_path(profile)).is_ok()
}

pub struct Pace {
    state: PathBuf,
    _lock: FileLock,
    fails: i64,
}

impl Pace {
    pub fn acquire(profile: &str, write: bool) -> Result<Pace> {
        let key = safe_key(profile);
        let dir = state_dir();
        let lock = FileLock::acquire(dir.join(format!("pace-{key}.lock")), 300_000)?;
        let state = dir.join(format!("pace-{key}.json"));
        let v = read_json(&state);
        let last = v["last_ms"].as_i64().unwrap_or(0);
        let fails = v["fails"].as_i64().unwrap_or(0);
        let (lo, hi) = if write { (3000, 6500) } else { (1200, 2800) };
        let mut gap = rand_range(lo, hi) as i64;
        if fails > 0 {
            let back = (2000i64 << (fails - 1).min(5)).min(60_000) + rand_range(0, 1500) as i64;
            gap = gap.max(back);
        }
        let wait = last + gap - now_ms();
        if wait > 0 {
            if wait > 5000 {
                eprintln!("xhs: 连续失败 {fails} 次，退避 {}s", wait / 1000);
            }
            sleep_ms(wait.min(120_000) as u64);
        }
        Ok(Pace { state, _lock: lock, fails })
    }

    pub fn finish(self, ok: bool) {
        let fails = if ok { 0 } else { self.fails + 1 };
        let _ = write_json(&self.state, &json!({"last_ms": now_ms(), "fails": fails}));
    }
}

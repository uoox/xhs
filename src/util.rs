use crate::error::{Result, XhsError};
use serde_json::{Map, Value};
use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

thread_local!(static SEED: Cell<u64> = const { Cell::new(0) });

pub fn rand_u64() -> u64 {
    SEED.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = (SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1) as u64) ^ ((std::process::id() as u64) << 32) | 1;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}

pub fn rand_range(lo: u64, hi: u64) -> u64 {
    if hi <= lo {
        return lo;
    }
    lo + rand_u64() % (hi - lo)
}

pub fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

pub fn state_dir() -> PathBuf {
    let d = std::env::var("XHS_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".cache/xhs"));
    let _ = fs::create_dir_all(&d);
    d
}

pub fn read_json(path: &PathBuf) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Object(Map::new()))
}

pub fn write_json(path: &PathBuf, v: &Value) -> Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, serde_json::to_vec_pretty(v).unwrap()).map_err(|e| XhsError::io(format!("写 {} 失败: {e}", tmp.display())))?;
    fs::rename(&tmp, path).map_err(|e| XhsError::io(format!("写 {} 失败: {e}", path.display())))
}

pub struct FileLock(PathBuf);

impl FileLock {
    pub fn acquire(path: PathBuf, wait_ms: u64) -> Result<FileLock> {
        let start = now_ms();
        loop {
            match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(FileLock(path)),
                Err(_) => {
                    let stale = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .map(|d| d.as_secs() > 150)
                        .unwrap_or(true);
                    if stale {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if (now_ms() - start) as u64 > wait_ms {
                        return Err(XhsError::new(
                            "busy",
                            format!("另一个 xhs 进程正在用同一个 profile（锁 {}）", path.display()),
                            "等它跑完再执行；确认没有 xhs 进程在跑时可删掉这个锁文件",
                        ));
                    }
                    sleep_ms(250);
                }
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn s(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        match v.get(*k) {
            Some(Value::String(x)) if !x.is_empty() => return x.clone(),
            Some(Value::Number(n)) => return n.to_string(),
            _ => {}
        }
    }
    String::new()
}

pub fn i(v: &Value, keys: &[&str]) -> i64 {
    for k in keys {
        match v.get(*k) {
            Some(Value::Number(n)) => return n.as_i64().unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as i64),
            Some(Value::String(x)) if !x.is_empty() => return count_num(x),
            _ => {}
        }
    }
    0
}

pub fn b(v: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|k| matches!(v.get(*k), Some(Value::Bool(true))))
}

pub fn o<'a>(v: &'a Value, keys: &[&str]) -> &'a Value {
    static NULL: Value = Value::Null;
    for k in keys {
        if let Some(x) = v.get(*k) {
            if !x.is_null() {
                return x;
            }
        }
    }
    &NULL
}

pub fn arr<'a>(v: &'a Value, keys: &[&str]) -> Vec<&'a Value> {
    match o(v, keys) {
        Value::Array(a) => a.iter().collect(),
        _ => vec![],
    }
}

pub fn count_num(t: &str) -> i64 {
    let t = t.trim();
    let mut num = String::new();
    let mut rest = "";
    for (idx, c) in t.char_indices() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
        } else {
            rest = &t[idx..];
            break;
        }
    }
    let n: f64 = num.parse().unwrap_or(0.0);
    let r = rest.trim().to_lowercase();
    let mul = if r.starts_with('万') || r.starts_with('w') {
        1e4
    } else if r.starts_with('k') || r.starts_with('千') {
        1e3
    } else if r.starts_with('亿') {
        1e8
    } else {
        1.0
    };
    (n * mul).round() as i64
}

pub fn id_time_ms(id: &str) -> i64 {
    if id.len() < 8 {
        return 0;
    }
    match i64::from_str_radix(&id[..8], 16) {
        Ok(n) if n > 1_400_000_000 && n < 4_000_000_000 => n * 1000,
        _ => 0,
    }
}

pub fn fmt_time(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let ms = if ms < 100_000_000_000 { ms * 1000 } else { ms };
    match chrono::DateTime::from_timestamp_millis(ms) {
        Some(t) => t
            .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => String::new(),
    }
}

pub fn parse_since(t: &str) -> Result<i64> {
    let t = t.trim();
    if t.is_empty() {
        return Err(XhsError::arg("--since 不能为空"));
    }
    let unit_ms = |u: &str| -> Option<i64> {
        Some(match u {
            "s" | "秒" => 1000,
            "m" | "min" | "分" | "分钟" => 60_000,
            "h" | "小时" => 3_600_000,
            "d" | "天" => 86_400_000,
            "w" | "周" => 7 * 86_400_000,
            _ => return None,
        })
    };
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &t[digits.len()..];
    if !digits.is_empty() {
        if rest.is_empty() {
            let n: i64 = digits.parse().map_err(|_| XhsError::arg("--since 数字太大"))?;
            return Ok(if n < 100_000_000_000 { n * 1000 } else { n });
        }
        if let Some(u) = unit_ms(rest) {
            let n: i64 = digits.parse().unwrap_or(0);
            return Ok(now_ms() - n * u);
        }
    }
    let tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(t) {
        return Ok(d.timestamp_millis());
    }
    for f in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(d) = chrono::NaiveDateTime::parse_from_str(t, f) {
            return Ok(d.and_local_timezone(tz).single().map(|x| x.timestamp_millis()).unwrap_or(0));
        }
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return Ok(d
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(tz)
            .single()
            .map(|x| x.timestamp_millis())
            .unwrap_or(0));
    }
    Err(XhsError::arg(format!(
        "看不懂 --since {t}：用 7d / 12h / 30m、秒或毫秒时间戳、2026-10-01 或 2026-10-01 08:00"
    )))
}

pub fn is_hex24(s: &str) -> bool {
    s.len() == 24 && s.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1.split('#').next()?;
    for kv in q.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        if k == key {
            return Some(url_decode(v));
        }
    }
    None
}

pub fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'%' if idx + 2 < bytes.len() => {
                if let Ok(n) = u8::from_str_radix(&s[idx + 1..idx + 3], 16) {
                    out.push(n);
                    idx += 3;
                    continue;
                }
                out.push(b'%');
                idx += 1;
            }
            b'+' => {
                out.push(b' ');
                idx += 1;
            }
            c => {
                out.push(c);
                idx += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct NoteRef {
    pub id: String,
    pub token: String,
    pub source: String,
}

pub fn parse_note(input: &str) -> Result<NoteRef> {
    let t = input.trim();
    if is_hex24(t) {
        return Ok(NoteRef {
            id: t.to_lowercase(),
            token: String::new(),
            source: String::new(),
        });
    }
    if t.starts_with("http") {
        if t.contains("xhslink.com") {
            return Err(XhsError::arg("不支持 xhslink 短链：先在浏览器打开它，复制跳转后的完整地址再传"));
        }
        let path = t.split(['?', '#']).next().unwrap_or("");
        let id = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
        if is_hex24(id) {
            return Ok(NoteRef {
                id: id.to_lowercase(),
                token: query_param(t, "xsec_token").unwrap_or_default(),
                source: query_param(t, "xsec_source").unwrap_or_default(),
            });
        }
    }
    Err(XhsError::arg(format!(
        "认不出笔记 {t}：传 24 位十六进制笔记 id，或带 xsec_token 的完整笔记链接"
    )))
}

pub fn parse_user(input: &str) -> Result<String> {
    let t = input.trim();
    if is_hex24(t) {
        return Ok(t.to_lowercase());
    }
    if t.starts_with("http") && t.contains("/user/profile/") {
        let path = t.split(['?', '#']).next().unwrap_or("");
        let id = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
        if is_hex24(id) {
            return Ok(id.to_lowercase());
        }
    }
    Err(XhsError::arg(format!(
        "认不出用户 {t}：传 24 位十六进制 user_id，或 https://www.xiaohongshu.com/user/profile/<id> 链接"
    )))
}

pub fn parse_comment_id(input: &str) -> Result<String> {
    let t = input.trim();
    if is_hex24(t) {
        Ok(t.to_lowercase())
    } else {
        Err(XhsError::arg(format!(
            "评论 id 应是 24 位十六进制，收到 {t}；从 `xhs comments <note> --json` 的 comment_id 取"
        )))
    }
}

pub fn norm(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        let c = match c as u32 {
            0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        };
        if c.is_whitespace() || matches!(c as u32, 0x200B..=0x200D | 0xFEFF) {
            continue;
        }
        if "·•・.。,，、_-—~～*＊|丨/／\\".contains(c) {
            continue;
        }
        out.extend(c.to_lowercase());
    }
    out
}

pub fn load_keywords(spec: &str) -> Result<Vec<String>> {
    let p = std::path::Path::new(spec);
    let raw = if p.is_file() {
        fs::read_to_string(p).map_err(|e| XhsError::io(format!("读关键词文件 {spec} 失败: {e}")))?
    } else {
        spec.to_string()
    };
    let words: Vec<String> = raw
        .split([',', '，', '\n', '\r'])
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty() && !w.starts_with('#'))
        .collect();
    if words.is_empty() {
        return Err(XhsError::arg("--match 没有有效关键词"));
    }
    Ok(words)
}

pub fn match_words(words: &[String], texts: &[&str]) -> Vec<String> {
    let t = texts.iter().map(|x| norm(x)).collect::<Vec<_>>().join("|");
    words
        .iter()
        .filter(|w| {
            let n = norm(w);
            !n.is_empty() && t.contains(&n)
        })
        .cloned()
        .collect()
}

pub fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

fn char_width(c: char) -> usize {
    let u = c as u32;
    if u < 0x20 || (0x300..0x370).contains(&u) || (0x200B..=0x200F).contains(&u) || (0xFE00..=0xFE0F).contains(&u) {
        0
    } else if (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE4F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x1F300..=0x1FAFF).contains(&u)
        || (0x20000..=0x3FFFD).contains(&u)
    {
        2
    } else {
        1
    }
}

pub fn clip(s: &str, max: usize) -> String {
    let flat: String = s.chars().map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).collect();
    if width(&flat) <= max {
        return flat;
    }
    let mut out = String::new();
    let mut w = 0;
    for c in flat.chars() {
        let cw = char_width(c);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

pub fn table(headers: &[&str], rows: &[Vec<String>], maxw: &[usize]) -> String {
    let n = headers.len();
    let mut ws: Vec<usize> = headers.iter().map(|h| width(h)).collect();
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            (0..n)
                .map(|k| clip(r.get(k).map(|x| x.as_str()).unwrap_or(""), *maxw.get(k).unwrap_or(&40)))
                .collect()
        })
        .collect();
    for r in &cells {
        for k in 0..n {
            ws[k] = ws[k].max(width(&r[k]));
        }
    }
    let line = |r: &[String]| -> String {
        let mut s = String::new();
        for k in 0..n {
            s.push_str(&r[k]);
            if k + 1 < n {
                s.push_str(&" ".repeat(ws[k] - width(&r[k]) + 2));
            }
        }
        s.trim_end().to_string()
    };
    let mut out = line(&headers.iter().map(|h| h.to_string()).collect::<Vec<_>>());
    out.push('\n');
    out.push_str(&ws.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("  "));
    for r in &cells {
        out.push('\n');
        out.push_str(&line(r));
    }
    out
}

use crate::amcu::Amcu;
use crate::error::{classify_api, risk_hint, Result, XhsError, HARD_RISK};
use crate::pace::{check_kicked, set_kicked, Pace};
use crate::util::{rand_range, read_json, sleep_ms, state_dir, write_json};
use serde_json::{json, Value};
use std::time::Duration;

const HOME_URL: &str = "https://www.xiaohongshu.com/explore";

const PRELUDE: &str = r#"
if(/website-login\/error/.test(location.href)){const u=new URL(location.href);return JSON.stringify({ok:false,reason:'risk_page',code:Number(u.searchParams.get('error_code'))||null,msg:u.searchParams.get('error_msg')||'',href:location.href})}
if(!/(^|\.)xiaohongshu\.com$/.test(location.hostname))return JSON.stringify({ok:false,reason:'no_app',href:location.href});
const chunk=window.webpackChunkxhs_pc_web;if(!Array.isArray(chunk))return JSON.stringify({ok:false,reason:'no_app',href:location.href});
const sleep=(ms)=>new Promise(r=>setTimeout(r,ms));
const apiErr=(e)=>{const d=e&&e.data;let code=d&&(d.code!==undefined?d.code:(d.result&&d.result.code));if(code===undefined||code===null)code=(e&&typeof e.code==='number')?e.code:null;return {ok:false,reason:'api',code,msg:String((d&&(d.msg||d.message))||(e&&(e.msg||e.message))||e),status:(e&&e.response&&e.response.status)||(e&&typeof e.status==='number'?e.status:null)}};
"#;

const CALL: &str = r#"
let req=null;try{chunk.push([['__xc'+Date.now()+Math.random()],{},(r)=>{req=r}])}catch(e){return JSON.stringify({ok:false,reason:'push_fail'})}
if(typeof req!=='function')return JSON.stringify({ok:false,reason:'no_req'});
const needles=['"'+P+'"',"'"+P+"'"];
const hit=(s)=>s.length<2000&&needles.some(n=>s.indexOf(n)!==-1)&&(!V||s.indexOf(V)!==-1);
const pick=(m)=>{if(!m||(typeof m!=='object'&&typeof m!=='function'))return null;let ks=[];try{ks=Object.keys(m)}catch(e){return null}for(const k of ks){let f;try{f=m[k]}catch(e){continue}if(typeof f!=='function')continue;let s='';try{s=Function.prototype.toString.call(f)}catch(e){continue}if(hit(s))return f}return null};
let fn=null;const cache=req.c||{};for(const id of Object.keys(cache)){fn=pick(cache[id]&&cache[id].exports);if(fn)break}
if(!fn){const fac=req.m||{};for(const id of Object.keys(fac)){let s='';try{s=String(fac[id])}catch(e){continue}if(!needles.some(n=>s.indexOf(n)!==-1))continue;let mod=null;try{mod=req(id)}catch(e){continue}fn=pick(mod);if(fn)break}}
if(!fn)return JSON.stringify({ok:false,reason:'no_wrapper'});
try{const r=await fn(...A);return JSON.stringify({ok:true,data:r===undefined?null:r})}catch(e){return JSON.stringify(apiErr(e))}
"#;

pub struct Page {
    pub amcu: Amcu,
    pub pkey: String,
    tab: Option<u64>,
    loaded: bool,
}

fn tabs_file() -> std::path::PathBuf {
    state_dir().join("tabs.json")
}

impl Page {
    pub fn new(profile: Option<String>, session: String) -> Page {
        let pkey = profile.clone().unwrap_or_default();
        Page {
            amcu: Amcu::new(profile, session),
            pkey,
            tab: None,
            loaded: false,
        }
    }

    fn tab_key(&self) -> String {
        format!("{}|{}", if self.pkey.is_empty() { "default" } else { &self.pkey }, self.amcu.session)
    }

    fn save_tab(&self) {
        let f = tabs_file();
        let mut v = read_json(&f);
        match self.tab {
            Some(t) => v[self.tab_key()] = json!(t),
            None => {
                if let Some(m) = v.as_object_mut() {
                    m.remove(&self.tab_key());
                }
            }
        }
        let _ = write_json(&f, &v);
    }

    fn raw_eval(&self, tab: u64, js: &str, timeout: u64) -> Result<Value> {
        let out = self.amcu.run(
            &["eval".into(), "--tab".into(), tab.to_string(), "--js".into(), js.to_string()],
            Duration::from_secs(timeout),
        )?;
        let mut v: Value = serde_json::from_str(out.trim()).unwrap_or(Value::String(out.trim().to_string()));
        for _ in 0..2 {
            if let Value::String(s) = &v {
                match serde_json::from_str::<Value>(s) {
                    Ok(x) => v = x,
                    Err(_) => break,
                }
            } else {
                break;
            }
        }
        Ok(v)
    }

    fn probe(&self, tab: u64) -> Result<Value> {
        self.raw_eval(tab, "JSON.stringify({href:location.href,host:location.hostname,app:Array.isArray(window.webpackChunkxhs_pc_web),ready:document.readyState,scripts:document.scripts.length})", 30)
    }

    fn risk_from_href(&self, href: &str) -> Option<XhsError> {
        if !href.contains("website-login/error") {
            return None;
        }
        let code = crate::util::query_param(href, "error_code").and_then(|c| c.parse::<i64>().ok());
        let msg = crate::util::query_param(href, "error_msg").unwrap_or_default();
        let reject = crate::util::query_param(href, "rejectUrl").unwrap_or_default();
        if code.map(|c| HARD_RISK.contains(&c)).unwrap_or(false) {
            return Some(self.risk(code, &msg, &format!("页面被重定向到安全限制页（{reject}）")));
        }
        let mut e = classify_api(&self.pkey, code, None, &msg, &format!("页面被重定向到安全限制页（{reject}）"));
        if e.code == "api_error" {
            e.code = "rate_limited";
            e.hint = "冷却 10 分钟以上再试；这是小红书的软限制，反复重试会升级".into();
        }
        Some(e)
    }

    fn risk(&self, code: Option<i64>, msg: &str, what: &str) -> XhsError {
        set_kicked(&self.pkey, code, msg);
        let mut e = XhsError::new(
            "risk_stop",
            format!("{what}：{} {msg}，已停止并锁定该 profile", code.map(|c| c.to_string()).unwrap_or_default()),
            risk_hint(if self.pkey.is_empty() { "<label>" } else { &self.pkey }),
        );
        e.api_code = code;
        e
    }

    pub fn ensure(&mut self) -> Result<u64> {
        if self.tab.is_none() {
            self.tab = read_json(&tabs_file())[self.tab_key()].as_u64();
        }
        if let Some(t) = self.tab {
            match self.probe(t) {
                Ok(v) => {
                    let href = v["href"].as_str().unwrap_or("");
                    if href.contains("website-login/error") {
                    } else if v["app"].as_bool() == Some(true) && v["host"].as_str().map(|h| h.ends_with("xiaohongshu.com")).unwrap_or(false) {
                        if !self.loaded {
                            self.loaded = true;
                        }
                        return Ok(t);
                    }
                    let _ = self
                        .amcu
                        .run(&["tab".into(), "--close".into(), "--tab".into(), t.to_string()], Duration::from_secs(20));
                }
                Err(e) if matches!(e.code, "ambiguous_profile" | "browser_unavailable" | "amcu_missing") => return Err(e),
                Err(_) => {}
            }
            self.tab = None;
            self.save_tab();
        }
        let out = self.amcu.run(
            &["tab".into(), "--new".into(), "--url".into(), HOME_URL.into(), "--timeout".into(), "40".into()],
            Duration::from_secs(70),
        )?;
        let id = out
            .split("opened tab ")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            .and_then(|x| x.parse::<u64>().ok())
            .ok_or_else(|| XhsError::new("amcu_error", format!("开标签页失败: {}", out.trim()), "`amcu browser doctor` 检查连接后重试"))?;
        self.tab = Some(id);
        self.save_tab();
        sleep_ms(rand_range(2500, 3800));
        let mut last = Value::Null;
        for _ in 0..8 {
            let v = self.probe(id)?;
            if let Some(e) = self.risk_from_href(v["href"].as_str().unwrap_or("")) {
                return Err(e);
            }
            if v["app"].as_bool() == Some(true) {
                self.loaded = true;
                return Ok(id);
            }
            last = v;
            sleep_ms(1200);
        }
        let _ = self.close();
        if last["ready"].as_str() == Some("complete") && last["scripts"].as_i64() == Some(0) {
            return Err(XhsError::new(
                "rate_limited",
                "小红书对这个账号返回了空白页面（HTTP 200 但正文为空），是服务端的软限制",
                "停止对这个 profile 的所有请求（包括面板），冷却 30 分钟以上再试；不要反复重开页面",
            ));
        }
        Err(XhsError::new(
            "page_error",
            "小红书页面打开了但没有 webpack 运行时（可能被重定向到登录页或网络问题）",
            "在该 profile 的 Chrome 里手动打开 xiaohongshu.com 看看是否正常、是否已登录",
        ))
    }

    pub fn close(&mut self) -> Result<bool> {
        if self.tab.is_none() {
            self.tab = read_json(&tabs_file())[self.tab_key()].as_u64();
        }
        let had = if let Some(t) = self.tab {
            self.amcu
                .run(&["tab".into(), "--close".into(), "--tab".into(), t.to_string()], Duration::from_secs(20))
                .is_ok()
        } else {
            false
        };
        self.tab = None;
        self.save_tab();
        Ok(had)
    }

    fn run_script(&mut self, body: &str, what: &str) -> Result<Value> {
        let js = format!("(async()=>{{{PRELUDE}{body}}})()");
        let mut retried = false;
        loop {
            let tab = self.ensure()?;
            let v = match self.raw_eval(tab, &js, 120) {
                Ok(v) => v,
                Err(e) if e.code == "tab_gone" && !retried => {
                    retried = true;
                    self.tab = None;
                    continue;
                }
                Err(e) => return Err(e),
            };
            if v["ok"].as_bool() == Some(true) {
                return Ok(v);
            }
            let reason = v["reason"].as_str().unwrap_or("");
            match reason {
                "risk_page" => return Err(self.risk(v["code"].as_i64(), v["msg"].as_str().unwrap_or(""), what)),
                "no_app" | "no_req" | "push_fail" | "no_wrapper" | "no_store" if !retried => {
                    retried = true;
                    if reason == "no_wrapper" || reason == "no_store" {
                        let _ = self.close();
                    }
                    self.tab = None;
                    continue;
                }
                "no_wrapper" => {
                    return Err(XhsError::new(
                        "no_wrapper",
                        format!("{what}：页面里找不到对应的接口封装函数"),
                        "可能小红书改版换了接口路径；`xhs close` 后重试一次，仍失败就需要更新 xhs（用 `xhs call` 自查路径）",
                    ))
                }
                "no_store" => {
                    return Err(XhsError::new(
                        "no_wrapper",
                        format!("{what}：页面里找不到对应的 pinia store {}", v["msg"].as_str().unwrap_or("")),
                        "可能小红书改版；`xhs close` 后重试一次，仍失败就需要更新 xhs",
                    ))
                }
                "api" => {
                    let code = v["code"].as_i64();
                    let status = v["status"].as_i64();
                    let msg = v["msg"].as_str().unwrap_or("");
                    let e = classify_api(&self.pkey, code, status, msg, what);
                    if e.code == "risk_stop" {
                        set_kicked(&self.pkey, e.api_code, msg);
                    }
                    if let Ok(p) = self.probe(tab) {
                        if let Some(r) = self.risk_from_href(p["href"].as_str().unwrap_or("")) {
                            return Err(r);
                        }
                    }
                    return Err(e);
                }
                _ => {
                    return Err(XhsError::new(
                        "page_error",
                        format!("{what}：页面返回异常 {}", v.to_string().chars().take(300).collect::<String>()),
                        "`xhs close` 后重试；持续出现请在 Chrome 里看看页面状态",
                    ))
                }
            }
        }
    }

    pub fn guarded(&mut self, write: bool, what: &str, body: &str) -> Result<Value> {
        check_kicked(&self.pkey)?;
        let pace = Pace::acquire(&self.pkey, write)?;
        let r = self.run_script(body, what);
        let ok = match &r {
            Ok(_) => true,
            Err(e) => matches!(e.code, "not_found" | "invalid_arg"),
        };
        pace.finish(ok);
        if let Err(e) = &r {
            if e.code == "risk_stop" || e.api_code.map(|c| HARD_RISK.contains(&c)).unwrap_or(false) {
                set_kicked(&self.pkey, e.api_code, &e.message);
            }
        }
        r
    }

    pub fn call(&mut self, path: &str, verb: &str, args: Value, write: bool, what: &str) -> Result<Value> {
        let body = format!(
            "const P={},V={},A={};{CALL}",
            serde_json::to_string(path).unwrap(),
            serde_json::to_string(verb).unwrap(),
            serde_json::to_string(&args).unwrap()
        );
        let v = self.guarded(write, what, &body)?;
        Ok(v["data"].clone())
    }

    pub fn get(&mut self, path: &str, params: Value, what: &str) -> Result<Value> {
        self.call(path, ".get(", json!([{ "params": params }]), false, what)
    }

    pub fn post(&mut self, path: &str, body: Value, write: bool, what: &str) -> Result<Value> {
        self.call(path, ".post(", json!([body]), write, what)
    }
}

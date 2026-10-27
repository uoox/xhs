mod amcu;
mod api;
mod dl;
mod error;
mod pace;
mod page;
mod util;

use clap::{Parser, Subcommand, ValueEnum};
use error::{Result, XhsError};
use page::Page;
use serde_json::{json, Value};
use std::path::PathBuf;
use util::*;

#[derive(Parser)]
#[command(
    name = "xhs",
    version,
    about = "小红书命令行：在已登录的 Chrome 页面里调用页面自己的接口封装（经 amcu），签名全由页面计算",
    after_help = "风控：所有请求之间随机间隔，连续失败指数退避；遇到 300011/300012/300015/461 立即停止并锁定该 profile。\n写操作一律要 --yes（或 --execute）才真发。\n--json 输出 {\"ok\":true,\"data\":…} 或 {\"ok\":false,\"error\":{code,message,hint}}。"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        env = "XHS_PROFILE",
        help = "amcu 的 profile（label / 目录名 / 显示名），`xhs profiles` 查看"
    )]
    profile: Option<String>,
    #[arg(
        long,
        global = true,
        env = "XHS_SESSION",
        default_value = "xhs-cli",
        help = "amcu 会话名，不同会话各自一个标签页"
    )]
    session: String,
    #[arg(long, global = true, help = "输出 JSON（给 agent / 面板用）")]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Sort {
    General,
    Time,
    Popular,
}

#[derive(Clone, Copy, ValueEnum)]
enum NoteKind {
    All,
    Video,
    Image,
}

#[derive(Clone, Copy, ValueEnum)]
enum NotifyKind {
    Mentions,
    Likes,
    Connections,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = "当前 profile 登录的小红书账号")]
    Whoami,
    #[command(about = "列出 amcu 连着的 Chrome profile")]
    Profiles,
    #[command(about = "某用户发布的笔记（点赞、时间、xsec_token）")]
    Notes {
        user: String,
        #[arg(long, help = "翻完所有页")]
        all: bool,
        #[arg(long, help = "只要这之后发布的：7d / 12h / 时间戳 / 2026-10-01")]
        since: Option<String>,
        #[arg(long, default_value_t = 30, help = "最多条数（--all/--since 时忽略）")]
        limit: usize,
    },
    #[command(about = "最近 N 天点赞最多的几篇（面板热门扫描用）")]
    Hot {
        user: String,
        #[arg(long, default_value_t = 7)]
        days: u32,
        #[arg(long, default_value_t = 3)]
        top: usize,
    },
    #[command(about = "笔记详情：标题、正文、图片、互动数、时间")]
    Note {
        note: String,
        #[arg(long)]
        xsec_token: Option<String>,
    },
    #[command(about = "评论（含二级评论、时间、IP 属地、图片、is_author）")]
    Comments {
        note: String,
        #[arg(long, default_value_t = 3, help = "一级评论页数（每页约 10 条）")]
        pages: usize,
        #[arg(long, help = "翻完所有页")]
        all: bool,
        #[arg(long, help = "展开二级评论的后续页")]
        sub: bool,
        #[arg(long, default_value_t = 3, help = "每条一级评论最多再拉几页二级评论（--all 时不限）")]
        sub_pages: usize,
        #[arg(long, help = "只输出这之后的评论：时间戳(秒/毫秒) / 2h / 7d / 2026-10-01")]
        since: Option<String>,
        #[arg(long = "match", help = "关键词：逗号分隔列表或文件路径（每行一个），只输出命中项并附 hits")]
        match_: Option<String>,
        #[arg(long, help = "命中判定时跳过作者自己的评论")]
        skip_author: bool,
        #[arg(long)]
        xsec_token: Option<String>,
    },
    #[command(about = "搜索笔记")]
    Search {
        keyword: String,
        #[arg(long, default_value_t = 1)]
        pages: usize,
        #[arg(long, value_enum, default_value = "general")]
        sort: Sort,
        #[arg(long = "type", value_enum, default_value = "all")]
        kind: NoteKind,
    },
    #[command(about = "用户资料")]
    User { user: String },
    #[command(about = "首页推荐流")]
    Feed {
        #[arg(long, default_value_t = 0, help = "再往下加载几批")]
        more: usize,
    },
    #[command(about = "点赞过的笔记（默认自己）")]
    Liked {
        user: Option<String>,
        #[arg(long, default_value_t = 1)]
        pages: usize,
    },
    #[command(about = "收藏的笔记（默认自己）")]
    Saved {
        user: Option<String>,
        #[arg(long, default_value_t = 1)]
        pages: usize,
    },
    #[command(about = "通知：评论和@ / 赞和收藏 / 新增关注")]
    Notifications {
        #[arg(long = "type", value_enum, default_value = "mentions")]
        kind: NotifyKind,
        #[arg(long, default_value_t = 1)]
        pages: usize,
        #[arg(long, default_value_t = 20)]
        num: usize,
        #[arg(long, help = "只看未读数")]
        unread: bool,
    },
    #[command(about = "评论写操作：post / delete / like / dislike")]
    Comment {
        #[command(subcommand)]
        action: CommentCmd,
    },
    #[command(about = "点赞笔记（--undo 取消）")]
    Like {
        note: String,
        #[arg(long)]
        undo: bool,
        #[arg(long, visible_alias = "execute")]
        yes: bool,
    },
    #[command(about = "收藏笔记（--undo 取消）")]
    Collect {
        note: String,
        #[arg(long)]
        undo: bool,
        #[arg(long, visible_alias = "execute")]
        yes: bool,
    },
    #[command(about = "关注用户")]
    Follow {
        user: String,
        #[arg(long, visible_alias = "execute")]
        yes: bool,
    },
    #[command(about = "取消关注")]
    Unfollow {
        user: String,
        #[arg(long, visible_alias = "execute")]
        yes: bool,
    },
    #[command(about = "备份一条评论：JSON + 图片 + 头像落到 <out>/<comment_id>/")]
    Backup {
        note: String,
        comment_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, help = "直接用这份评论 JSON（文件或 - 读 stdin），不再请求小红书")]
        from: Option<String>,
        #[arg(long, default_value_t = 20, help = "在线查找时最多翻几页")]
        pages: usize,
        #[arg(long)]
        xsec_token: Option<String>,
    },
    #[command(about = "下载图片（带 referer），--out 是文件或目录")]
    Img {
        url: String,
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    #[command(about = "直接调用页面里任一接口封装（按路径找），POST 需 --yes")]
    Call {
        path: String,
        #[arg(
            long,
            default_value = "[]",
            help = "传给封装函数的参数数组 JSON，GET 一般是 [{\"params\":{…}}]，POST 是 [{camelCase body}]"
        )]
        args: String,
        #[arg(long, default_value = "auto", help = "get / post / any / auto（auto 时有 params 视为 get）")]
        verb: String,
        #[arg(long, visible_alias = "execute")]
        yes: bool,
    },
    #[command(about = "风控锁定后，确认已在 Chrome 重新登录再解锁")]
    Unlock,
    #[command(about = "关掉 xhs 在该 profile 开的标签页")]
    Close,
}

#[derive(Subcommand)]
enum CommentCmd {
    #[command(about = "发评论（--reply-to 回复某条）")]
    Post {
        note: String,
        text: String,
        #[arg(long)]
        reply_to: Option<String>,
        #[arg(long, visible_alias = "execute")]
        yes: bool,
    },
    #[command(about = "删评论（自己的评论，或自己笔记下的任意评论）")]
    Delete {
        note: String,
        comment_id: String,
        #[arg(long, visible_alias = "execute")]
        yes: bool,
    },
    #[command(about = "给评论点赞")]
    Like {
        note: String,
        comment_id: String,
        #[arg(long, visible_alias = "execute")]
        yes: bool,
    },
    #[command(about = "取消评论点赞")]
    Dislike {
        note: String,
        comment_id: String,
        #[arg(long, visible_alias = "execute")]
        yes: bool,
    },
}

struct Out {
    data: Value,
    text: String,
}

fn out(data: Value, text: String) -> Result<Out> {
    Ok(Out { data, text })
}

fn main() {
    let cli = Cli::parse();
    let json_mode = cli.json;
    match run(cli) {
        Ok(o) => {
            if json_mode {
                println!("{}", serde_json::to_string(&json!({"ok": true, "data": o.data})).unwrap());
            } else if !o.text.is_empty() {
                println!("{}", o.text);
            }
        }
        Err(e) => {
            if json_mode {
                println!("{}", serde_json::to_string(&e.to_json()).unwrap());
            } else {
                eprintln!("{e}");
            }
            std::process::exit(e.exit_code());
        }
    }
}

fn notes_table(v: &[Value]) -> String {
    let rows: Vec<Vec<String>> = v
        .iter()
        .map(|n| {
            vec![
                fmt_time(n["time"].as_i64().unwrap_or(0)),
                n["likes"].as_str().unwrap_or("").to_string(),
                if n["sticky"].as_bool() == Some(true) {
                    "置顶".into()
                } else {
                    String::new()
                },
                n["nickname"].as_str().unwrap_or("").to_string(),
                n["title"].as_str().unwrap_or("").to_string(),
                n["note_id"].as_str().unwrap_or("").to_string(),
            ]
        })
        .collect();
    table(&["发布时间", "点赞", "", "作者", "标题", "note_id"], &rows, &[16, 8, 4, 14, 40, 24])
}

fn comments_table(v: &[Value], with_hits: bool) -> String {
    let rows: Vec<Vec<String>> = v
        .iter()
        .map(|c| {
            let mut content = c["content"].as_str().unwrap_or("").to_string();
            let pics = c["pictures"].as_array().map(|a| a.len()).unwrap_or(0);
            if pics > 0 {
                content.push_str(&format!(" [图{pics}]"));
            }
            let mut r = vec![
                fmt_time(c["create_time"].as_i64().unwrap_or(0)),
                if c["parent_id"].as_str().unwrap_or("").is_empty() {
                    "┌".into()
                } else {
                    "└".into()
                },
                format!(
                    "{}{}",
                    c["nick"].as_str().unwrap_or(""),
                    if c["is_author"].as_bool() == Some(true) { "(作者)" } else { "" }
                ),
                c["ip_location"].as_str().unwrap_or("").to_string(),
                c["like_count"].as_str().unwrap_or("").to_string(),
                content,
                c["comment_id"].as_str().unwrap_or("").to_string(),
            ];
            if with_hits {
                r.push(
                    c["hits"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(","))
                        .unwrap_or_default(),
                );
            }
            r
        })
        .collect();
    if with_hits {
        table(
            &["时间", "", "昵称", "属地", "赞", "内容", "comment_id", "命中"],
            &rows,
            &[16, 2, 16, 6, 5, 50, 24, 20],
        )
    } else {
        table(&["时间", "", "昵称", "属地", "赞", "内容", "comment_id"], &rows, &[16, 2, 16, 6, 5, 60, 24])
    }
}

fn kv(pairs: &[(&str, String)]) -> String {
    let w = pairs.iter().map(|(k, _)| width(k)).max().unwrap_or(0);
    pairs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{}{}  {}", k, " ".repeat(w - width(k)), v))
        .collect::<Vec<_>>()
        .join("\n")
}

fn vs(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        x => x.to_string(),
    }
}

fn need_yes(yes: bool, action: &str, plan: Value) -> Result<()> {
    if yes {
        return Ok(());
    }
    let mut e = XhsError::confirm(action);
    e.message = format!("{}；计划: {}", e.message, plan);
    Err(e)
}

fn actor(p: &mut Page) -> Result<String> {
    let me = api::require_login(p)?;
    Ok(format!("{}({})", vs(&me, "nickname"), vs(&me, "user_id")))
}

fn load_comment_json(spec: &str) -> Result<Value> {
    let raw = if spec == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| XhsError::io(format!("读 stdin 失败: {e}")))?;
        s
    } else {
        std::fs::read_to_string(spec).map_err(|e| XhsError::io(format!("读 {spec} 失败: {e}")))?
    };
    let mut v: Value = serde_json::from_str(&raw).map_err(|e| XhsError::arg(format!("--from 不是合法 JSON: {e}")))?;
    if let Some(d) = v.get("data").cloned() {
        if d.is_object() {
            v = d;
        }
    }
    if let Some(Value::String(p)) = v.get("pictures").cloned() {
        v["pictures"] = serde_json::from_str(&p).unwrap_or(json!([]));
    }
    Ok(v)
}

fn run(cli: Cli) -> Result<Out> {
    let mut p = Page::new(cli.profile.clone(), cli.session.clone());
    let is_saved = matches!(&cli.cmd, Cmd::Saved { .. });
    let is_follow = matches!(&cli.cmd, Cmd::Follow { .. });
    let is_like = matches!(
        &cli.cmd,
        Cmd::Comment {
            action: CommentCmd::Like { .. }
        }
    );
    match cli.cmd {
        Cmd::Whoami => {
            let me = api::whoami(&mut p)?;
            let text = if me["logged_in"].as_bool() == Some(true) {
                kv(&[
                    ("昵称", vs(&me, "nickname")),
                    ("user_id", vs(&me, "user_id")),
                    ("小红书号", vs(&me, "red_id")),
                    ("profile", vs(&me, "profile")),
                ])
            } else {
                "未登录（游客态）：在该 profile 的 Chrome 里登录 xiaohongshu.com".into()
            };
            out(me, text)
        }
        Cmd::Profiles => {
            let o = p.amcu.exec(&["profiles".into()], std::time::Duration::from_secs(20))?;
            let raw = format!("{}{}", o.stdout, o.stderr);
            if !o.ok && o.stdout.trim().is_empty() {
                return Err(p.amcu.map_err(&raw));
            }
            let mut list = vec![];
            for l in o.stdout.lines().filter(|l| l.contains("pid=")) {
                let get = |k: &str| -> String {
                    l.split_whitespace()
                        .find_map(|t| t.strip_prefix(&format!("{k}=")))
                        .unwrap_or("")
                        .trim_matches('"')
                        .to_string()
                };
                let dir = l.split("profile=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
                let name = l
                    .split("profile=\"")
                    .nth(1)
                    .and_then(|r| r.split_once('"'))
                    .and_then(|(_, rest)| rest.trim_start().strip_prefix('('))
                    .and_then(|r| r.split(")\t").next())
                    .unwrap_or("")
                    .to_string();
                list.push(json!({"browser": l.split_whitespace().next().unwrap_or(""), "pid": get("pid"), "dir": dir, "name": name, "label": get("label"), "instance": get("instance"), "tabs": get("tabs"), "since": get("since")}));
            }
            out(json!(list), o.stdout.trim_end().to_string())
        }
        Cmd::Notes { user, all, since, limit } => {
            let uid = parse_user(&user)?;
            let since_ms = since.as_deref().map(parse_since).transpose()?;
            let (notes, more) = api::user_posted(&mut p, &uid, limit, since_ms, all)?;
            let text = format!(
                "{}\n共 {} 篇{}",
                notes_table(&notes),
                notes.len(),
                if more && !all && since_ms.is_none() {
                    "（还有更多，加 --all）"
                } else {
                    ""
                }
            );
            out(json!({"user_id": uid, "has_more": more, "notes": notes}), text)
        }
        Cmd::Hot { user, days, top } => {
            let uid = parse_user(&user)?;
            let notes = api::hot(&mut p, &uid, days, top)?;
            let text = format!("{}\n最近 {days} 天点赞前 {top}：{} 篇", notes_table(&notes), notes.len());
            out(json!({"user_id": uid, "days": days, "notes": notes}), text)
        }
        Cmd::Note { note, xsec_token } => {
            let mut n = parse_note(&note)?;
            api::resolve(&mut n, xsec_token.as_ref())?;
            let d = api::note_detail(&mut p, &n)?;
            let imgs = d["images"].as_array().map(|a| a.len()).unwrap_or(0);
            let text = kv(&[
                ("标题", vs(&d, "title")),
                ("作者", format!("{} ({})", vs(&d, "nickname"), vs(&d, "user_id"))),
                ("发布时间", fmt_time(d["time"].as_i64().unwrap_or(0))),
                ("IP 属地", vs(&d, "ip_location")),
                (
                    "互动",
                    format!(
                        "赞 {}  藏 {}  评 {}  转 {}",
                        vs(&d, "likes"),
                        vs(&d, "collects"),
                        vs(&d, "comments"),
                        vs(&d, "shares")
                    ),
                ),
                (
                    "类型",
                    format!("{}{}", vs(&d, "type"), if imgs > 0 { format!("，{imgs} 张图") } else { String::new() }),
                ),
                (
                    "标签",
                    d["tags"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" "))
                        .unwrap_or_default(),
                ),
                ("链接", vs(&d, "url")),
                ("正文", vs(&d, "desc")),
            ]);
            out(d, text)
        }
        Cmd::Comments {
            note,
            pages,
            all,
            sub,
            sub_pages,
            since,
            match_,
            skip_author,
            xsec_token,
        } => {
            let mut n = parse_note(&note)?;
            let since_ms = since.as_deref().map(parse_since).transpose()?;
            let words = match_.as_deref().map(load_keywords).transpose()?;
            api::resolve(&mut n, xsec_token.as_ref())?;
            let opt = api::CommentOpts {
                pages: if all { 10_000 } else { pages.max(1) },
                sub,
                sub_pages: if all { 10_000 } else { sub_pages },
                stop_at: None,
            };
            let mut d = api::comments(&mut p, &n, &opt)?;
            let total = d["comments"].as_array().map(|a| a.len()).unwrap_or(0);
            let mut list: Vec<Value> = d["comments"].as_array().cloned().unwrap_or_default();
            if let Some(t) = since_ms {
                list.retain(|c| c["create_time"].as_i64().unwrap_or(0) >= t);
            }
            if let Some(w) = &words {
                list = list
                    .into_iter()
                    .filter_map(|mut c| {
                        if skip_author && c["is_author"].as_bool() == Some(true) {
                            return None;
                        }
                        let hits = match_words(w, &[c["content"].as_str().unwrap_or(""), c["nick"].as_str().unwrap_or("")]);
                        if hits.is_empty() {
                            None
                        } else {
                            c["hits"] = json!(hits);
                            Some(c)
                        }
                    })
                    .collect();
            }
            let text = format!(
                "{}\n抓取 {} 页、{} 条，输出 {} 条{}",
                comments_table(&list, words.is_some()),
                d["pages"],
                total,
                list.len(),
                if d["has_more"].as_bool() == Some(true) {
                    "（还有更多页，加 --pages N 或 --all）"
                } else {
                    ""
                }
            );
            d["fetched"] = json!(total);
            d["comments"] = json!(list);
            if let Some(w) = words {
                d["keywords"] = json!(w);
            }
            if let Some(t) = since_ms {
                d["since"] = json!(t);
            }
            out(d, text)
        }
        Cmd::Search { keyword, pages, sort, kind } => {
            if keyword.trim().is_empty() {
                return Err(XhsError::arg("关键词不能为空"));
            }
            let sort_s = match sort {
                Sort::General => "general",
                Sort::Time => "time_descending",
                Sort::Popular => "popularity_descending",
            };
            let t = match kind {
                NoteKind::All => 0,
                NoteKind::Video => 1,
                NoteKind::Image => 2,
            };
            let (r, more) = api::search(&mut p, keyword.trim(), pages, sort_s, t)?;
            let text = format!("{}\n共 {} 篇{}", notes_table(&r), r.len(), if more { "（还有更多，加 --pages）" } else { "" });
            out(json!({"keyword": keyword, "has_more": more, "notes": r}), text)
        }
        Cmd::User { user } => {
            let uid = parse_user(&user)?;
            let d = api::user_info(&mut p, &uid)?;
            let text = kv(&[
                ("昵称", vs(&d, "nickname")),
                ("user_id", vs(&d, "user_id")),
                ("小红书号", vs(&d, "red_id")),
                ("IP 属地", vs(&d, "ip_location")),
                (
                    "关注/粉丝/获赞藏",
                    format!("{} / {} / {}", vs(&d, "follows"), vs(&d, "fans"), vs(&d, "interaction")),
                ),
                ("关注状态", vs(&d, "followed")),
                ("简介", vs(&d, "desc")),
                ("主页", vs(&d, "url")),
            ]);
            out(d, text)
        }
        Cmd::Feed { more } => {
            let r = api::feed(&mut p, more)?;
            let text = format!("{}\n共 {} 篇", notes_table(&r), r.len());
            out(json!({"notes": r}), text)
        }
        Cmd::Liked { user, pages } | Cmd::Saved { user, pages } => {
            let saved = is_saved;
            let uid = match user {
                Some(u) => parse_user(&u)?,
                None => vs(&api::require_login(&mut p)?, "user_id"),
            };
            let (r, more) = api::liked_or_saved(&mut p, &uid, saved, pages)?;
            let text = format!("{}\n共 {} 篇{}", notes_table(&r), r.len(), if more { "（还有更多，加 --pages）" } else { "" });
            out(json!({"user_id": uid, "has_more": more, "notes": r}), text)
        }
        Cmd::Notifications { kind, pages, num, unread } => {
            if unread {
                let d = api::unread(&mut p)?;
                let text = format!(
                    "未读 {}：评论和@ {}，赞和收藏 {}，新增关注 {}",
                    d["unread"], d["mentions"], d["likes"], d["connections"]
                );
                return out(d, text);
            }
            let k = match kind {
                NotifyKind::Mentions => "mentions",
                NotifyKind::Likes => "likes",
                NotifyKind::Connections => "connections",
            };
            let (r, more) = api::notifications(&mut p, k, num.clamp(1, 50), pages)?;
            let rows: Vec<Vec<String>> = r
                .iter()
                .map(|m| {
                    vec![
                        fmt_time(m["time"].as_i64().unwrap_or(0)),
                        vs(m, "title"),
                        vs(m, "nickname"),
                        vs(m, "comment"),
                        vs(m, "note_title"),
                        vs(m, "note_id"),
                    ]
                })
                .collect();
            let text = format!(
                "{}\n共 {} 条",
                table(&["时间", "类型", "用户", "内容", "笔记", "note_id"], &rows, &[16, 12, 14, 40, 20, 24]),
                r.len()
            );
            out(json!({"type": k, "has_more": more, "items": r}), text)
        }
        Cmd::Comment { action } => match action {
            CommentCmd::Post { note, text, reply_to, yes } => {
                let n = parse_note(&note)?;
                let r = reply_to.as_deref().map(parse_comment_id).transpose()?;
                if text.trim().is_empty() {
                    return Err(XhsError::arg("评论内容不能为空"));
                }
                if text.chars().count() > 1000 {
                    return Err(XhsError::arg("评论太长（>1000 字）"));
                }
                need_yes(yes, "发评论", json!({"note_id": n.id, "text": text, "reply_to": r}))?;
                let who = actor(&mut p)?;
                let d = api::comment_post(&mut p, &n.id, &text, r.as_deref())?;
                let t = format!("已以 {who} 发表评论 comment_id={}", vs(&d, "comment_id"));
                out(d, t)
            }
            CommentCmd::Delete { note, comment_id, yes } => {
                let n = parse_note(&note)?;
                let c = parse_comment_id(&comment_id)?;
                need_yes(yes, "删评论", json!({"note_id": n.id, "comment_id": c}))?;
                let who = actor(&mut p)?;
                api::write_simple(&mut p, "/api/sns/web/v1/comment/delete", json!({"noteId": n.id, "commentId": c}), "删评论")?;
                out(
                    json!({"note_id": n.id, "comment_id": c, "deleted": true, "by": who}),
                    format!("已以 {who} 删除评论 {c}"),
                )
            }
            CommentCmd::Like { note, comment_id, yes } | CommentCmd::Dislike { note, comment_id, yes } => {
                let n = parse_note(&note)?;
                let c = parse_comment_id(&comment_id)?;
                let (path, what) = if is_like {
                    ("/api/sns/web/v1/comment/like", "评论点赞")
                } else {
                    ("/api/sns/web/v1/comment/dislike", "取消评论点赞")
                };
                need_yes(yes, what, json!({"note_id": n.id, "comment_id": c}))?;
                let who = actor(&mut p)?;
                api::write_simple(&mut p, path, json!({"noteId": n.id, "commentId": c}), what)?;
                out(
                    json!({"note_id": n.id, "comment_id": c, "action": what, "by": who}),
                    format!("已以 {who} {what} {c}"),
                )
            }
        },
        Cmd::Like { note, undo, yes } => {
            let n = parse_note(&note)?;
            let (path, what) = if undo {
                ("/api/sns/web/v1/note/dislike", "取消点赞笔记")
            } else {
                ("/api/sns/web/v1/note/like", "点赞笔记")
            };
            need_yes(yes, what, json!({"note_id": n.id}))?;
            let who = actor(&mut p)?;
            api::write_simple(&mut p, path, json!({"noteOid": n.id}), what)?;
            out(json!({"note_id": n.id, "action": what, "by": who}), format!("已以 {who} {what} {}", n.id))
        }
        Cmd::Collect { note, undo, yes } => {
            let n = parse_note(&note)?;
            let what = if undo { "取消收藏笔记" } else { "收藏笔记" };
            need_yes(yes, what, json!({"note_id": n.id}))?;
            let who = actor(&mut p)?;
            if undo {
                api::write_simple(&mut p, "/api/sns/web/v1/note/uncollect", json!({"noteIds": n.id}), what)?;
            } else {
                api::write_simple(&mut p, "/api/sns/web/v1/note/collect", json!({"noteId": n.id}), what)?;
            }
            out(json!({"note_id": n.id, "action": what, "by": who}), format!("已以 {who} {what} {}", n.id))
        }
        Cmd::Follow { user, yes } | Cmd::Unfollow { user, yes } => {
            let follow = is_follow;
            let uid = parse_user(&user)?;
            let (path, what) = if follow {
                ("/api/sns/web/v1/user/follow", "关注")
            } else {
                ("/api/sns/web/v1/user/unfollow", "取消关注")
            };
            need_yes(yes, what, json!({"user_id": uid}))?;
            let who = actor(&mut p)?;
            api::write_simple(&mut p, path, json!({"targetUserId": uid}), what)?;
            out(json!({"user_id": uid, "action": what, "by": who}), format!("已以 {who} {what} {uid}"))
        }
        Cmd::Backup {
            note,
            comment_id,
            out: dir,
            from,
            pages,
            xsec_token,
        } => {
            let mut n = parse_note(&note)?;
            let cid = parse_comment_id(&comment_id)?;
            let c = match from {
                Some(f) => {
                    let v = load_comment_json(&f)?;
                    if v["comment_id"].as_str() != Some(cid.as_str()) {
                        return Err(XhsError::arg(format!(
                            "--from 里的 comment_id 是 {}，与参数 {cid} 不一致",
                            vs(&v, "comment_id")
                        )));
                    }
                    v
                }
                None => {
                    api::resolve(&mut n, xsec_token.as_ref())?;
                    let d = api::comments(
                        &mut p,
                        &n,
                        &api::CommentOpts {
                            pages: pages.max(1),
                            sub: true,
                            sub_pages: 5,
                            stop_at: Some(cid.clone()),
                        },
                    )?;
                    d["comments"]
                        .as_array()
                        .and_then(|a| a.iter().find(|c| c["comment_id"].as_str() == Some(cid.as_str())).cloned())
                        .ok_or_else(|| {
                            XhsError::new(
                                "not_found",
                                format!("翻了 {} 页没找到评论 {cid}", d["pages"]),
                                "评论可能已删除；或加大 --pages；面板里已有评论 JSON 时用 --from",
                            )
                        })?
                }
            };
            let target = dir.join(&cid);
            std::fs::create_dir_all(&target).map_err(|e| XhsError::io(format!("建目录 {} 失败: {e}", target.display())))?;
            let mut files = vec![];
            let mut errors = vec![];
            for (k, u) in c["pictures"].as_array().cloned().unwrap_or_default().iter().enumerate() {
                if let Some(u) = u.as_str() {
                    match dl::save(u, &target, &format!("pic_{k}")) {
                        Ok(f) => files.push(f.file_name().unwrap().to_string_lossy().into_owned()),
                        Err(e) => errors.push(e.message),
                    }
                    sleep_ms(rand_range(200, 600));
                }
            }
            if let Some(a) = c["avatar"].as_str().filter(|a| !a.is_empty()) {
                match dl::save(a, &target, "avatar") {
                    Ok(f) => files.push(f.file_name().unwrap().to_string_lossy().into_owned()),
                    Err(e) => errors.push(e.message),
                }
            }
            let mut rec = c.clone();
            rec["note_id"] = json!(n.id);
            rec["files"] = json!(files);
            rec["backup_at"] = json!(chrono::Utc::now().to_rfc3339());
            if !errors.is_empty() {
                rec["download_errors"] = json!(errors);
            }
            write_json(&target.join("comment.json"), &rec)?;
            let text = format!(
                "已备份到 {}：comment.json + {} 个文件{}",
                target.display(),
                files.len(),
                if errors.is_empty() {
                    String::new()
                } else {
                    format!("，{} 个下载失败", errors.len())
                }
            );
            out(json!({"dir": target, "files": files, "errors": errors, "comment": rec}), text)
        }
        Cmd::Img { url, out: o } => {
            let stem = dl::url_stem(&url);
            let f = dl::save(&url, &o, &stem)?;
            let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
            out(json!({"file": f, "bytes": size}), format!("已保存 {}（{} 字节）", f.display(), size))
        }
        Cmd::Call { path, args, verb, yes } => {
            if !path.starts_with("/api/") {
                return Err(XhsError::arg("路径要以 /api/ 开头，例如 /api/sns/web/v1/user/otherinfo"));
            }
            let a: Value = serde_json::from_str(&args).map_err(|e| XhsError::arg(format!("--args 不是合法 JSON: {e}")))?;
            let a = if a.is_array() { a } else { json!([a]) };
            let v = match verb.as_str() {
                "get" => ".get(",
                "post" => ".post(",
                "any" => "",
                _ => {
                    if a[0].get("params").is_some() || a.as_array().map(|x| x.is_empty()).unwrap_or(true) {
                        ".get("
                    } else {
                        ".post("
                    }
                }
            };
            let readonly_post = [
                "/api/sns/web/v1/feed",
                "/api/sns/web/v1/homefeed",
                "/api/sns/web/v1/search/usersearch",
                "/api/sns/web/v1/search/onebox",
            ];
            let write = v != ".get(" && !readonly_post.contains(&path.as_str());
            if write {
                need_yes(yes, &format!("调用 {path}"), json!({"path": path, "args": a}))?;
            }
            let d = p.call(&path, v, a, write, &format!("调用 {path}"))?;
            let text = serde_json::to_string_pretty(&d).unwrap();
            out(d, text)
        }
        Cmd::Unlock => {
            let had = pace::unlock(&p.pkey);
            out(
                json!({"profile": p.pkey, "unlocked": had}),
                if had {
                    format!("已解锁 profile {}，先跑 `xhs whoami` 确认登录正常", p.pkey)
                } else {
                    "该 profile 没有被锁".into()
                },
            )
        }
        Cmd::Close => {
            let had = p.close()?;
            out(
                json!({"closed": had}),
                if had {
                    "已关闭标签页".into()
                } else {
                    "没有记录中的标签页".into()
                },
            )
        }
    }
}

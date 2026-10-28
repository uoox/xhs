use crate::error::{Result, XhsError};
use crate::page::Page;
use crate::util::*;
use serde_json::{json, Value};

const IMG_FMT: &str = "jpg,webp,avif";

fn tokens_file() -> std::path::PathBuf {
    state_dir().join("tokens.json")
}

pub fn remember(items: &[(String, String, String)]) {
    let items: Vec<_> = items.iter().filter(|(id, t, _)| !id.is_empty() && !t.is_empty()).collect();
    if items.is_empty() {
        return;
    }
    let f = tokens_file();
    let mut v = read_json(&f);
    if !v.is_object() {
        v = json!({});
    }
    let now = now_ms();
    for (id, t, src) in items {
        v[id.as_str()] = json!({"t": t, "s": src, "at": now});
    }
    if let Some(m) = v.as_object_mut() {
        if m.len() > 6000 {
            let mut all: Vec<(String, i64)> = m.iter().map(|(k, x)| (k.clone(), x["at"].as_i64().unwrap_or(0))).collect();
            all.sort_by_key(|x| x.1);
            for (k, _) in all.iter().take(m.len() - 5000) {
                m.remove(k);
            }
        }
    }
    let _ = write_json(&f, &v);
}

pub fn lookup(id: &str) -> Option<(String, String)> {
    let v = read_json(&tokens_file());
    let t = v[id]["t"].as_str()?.to_string();
    Some((t, v[id]["s"].as_str().unwrap_or("").to_string()))
}

pub fn resolve(n: &mut NoteRef, explicit: Option<&String>) -> Result<()> {
    if let Some(t) = explicit {
        n.token = t.clone();
    }
    if n.token.is_empty() {
        if let Some((t, s)) = lookup(&n.id) {
            n.token = t;
            if n.source.is_empty() {
                n.source = s;
            }
        }
    }
    if n.token.is_empty() {
        return Err(XhsError::new(
            "need_xsec_token",
            format!("笔记 {} 没有 xsec_token，本地缓存里也没有", n.id),
            "先跑一次 `xhs notes <作者id>` / `xhs search <词>` / `xhs feed`（会自动缓存 token），或直接传带 xsec_token 的完整笔记链接，或加 --xsec-token",
        ));
    }
    if n.source.is_empty() {
        n.source = "pc_feed".into();
    }
    Ok(())
}

fn cover_url(c: &Value) -> String {
    let u = s(c, &["urlDefault", "url_default", "urlPre", "url"]);
    if !u.is_empty() {
        return u;
    }
    for x in arr(c, &["infoList", "info_list"]) {
        if s(x, &["imageScene", "image_scene"]) == "WB_DFT" {
            return s(x, &["url"]);
        }
    }
    arr(c, &["infoList", "info_list"]).first().map(|x| s(x, &["url"])).unwrap_or_default()
}

fn card_brief(id: &str, token: &str, card: &Value, source: &str) -> Value {
    let ii = o(card, &["interactInfo", "interact_info"]);
    let user = o(card, &["user"]);
    let mut t = i(card, &["time", "publishTime"]);
    if t == 0 {
        t = id_time_ms(id);
    }
    json!({
        "note_id": id,
        "title": s(card, &["displayTitle", "display_title", "title"]),
        "type": s(card, &["type"]),
        "likes": s(ii, &["likedCount", "liked_count"]),
        "likes_n": i(ii, &["likedCount", "liked_count"]),
        "collects": s(ii, &["collectedCount", "collected_count"]),
        "comments": s(ii, &["commentCount", "comment_count"]),
        "sticky": b(ii, &["sticky"]),
        "time": t,
        "user_id": s(user, &["userId", "user_id"]),
        "nickname": s(user, &["nickname", "nickName", "nick_name"]),
        "cover": cover_url(o(card, &["cover"])),
        "xsec_token": token,
        "xsec_source": source,
        "url": note_url(id, token, source),
    })
}

pub fn note_url(id: &str, token: &str, source: &str) -> String {
    if token.is_empty() {
        format!("https://www.xiaohongshu.com/explore/{id}")
    } else {
        format!(
            "https://www.xiaohongshu.com/explore/{id}?xsec_token={}&xsec_source={}",
            token.replace('=', "%3D"),
            source
        )
    }
}

pub fn whoami(p: &mut Page) -> Result<Value> {
    let d = p.get("/api/sns/web/v2/user/me", json!({}), "读取当前账号")?;
    let uid = s(&d, &["userId", "user_id"]);
    let guest = b(&d, &["guest"]);
    Ok(json!({
        "user_id": uid,
        "nickname": s(&d, &["nickname"]),
        "red_id": s(&d, &["redId", "red_id"]),
        "desc": s(&d, &["desc"]),
        "avatar": s(&d, &["images", "imageb", "image"]),
        "guest": guest,
        "logged_in": !uid.is_empty() && !guest,
        "profile": p.pkey,
    }))
}

pub fn require_login(p: &mut Page) -> Result<Value> {
    let me = whoami(p)?;
    if me["logged_in"].as_bool() != Some(true) {
        return Err(XhsError::new(
            "not_logged_in",
            format!(
                "profile {} 的 Chrome 没有登录小红书（游客态）",
                if p.pkey.is_empty() { "default" } else { &p.pkey }
            ),
            "在该 profile 的 Chrome 里打开 xiaohongshu.com 扫码登录，再重试",
        ));
    }
    Ok(me)
}

pub fn user_posted(p: &mut Page, uid: &str, limit: usize, since_ms: Option<i64>, all: bool) -> Result<(Vec<Value>, bool)> {
    let mut out: Vec<Value> = vec![];
    let mut cursor = String::new();
    let mut more;
    let mut seen = std::collections::HashSet::new();
    loop {
        let d = p.get(
            "/api/sns/web/v1/user_posted",
            json!({"num": 30, "cursor": cursor, "user_id": uid, "image_formats": IMG_FMT, "xsec_token": "", "xsec_source": "pc_user"}),
            "读取用户笔记列表",
        )?;
        let mut stop = false;
        for n in arr(&d, &["notes"]) {
            let id = s(n, &["noteId", "note_id"]);
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            let tok = s(n, &["xsecToken", "xsec_token"]);
            let v = card_brief(&id, &tok, n, "pc_user");
            if let Some(since) = since_ms {
                if v["time"].as_i64().unwrap_or(0) < since {
                    if !v["sticky"].as_bool().unwrap_or(false) {
                        stop = true;
                    }
                    continue;
                }
            }
            out.push(v);
        }
        more = b(&d, &["hasMore", "has_more"]);
        cursor = s(&d, &["cursor"]);
        if stop || !more || cursor.is_empty() || (!all && since_ms.is_none() && out.len() >= limit) || out.len() >= 3000 {
            break;
        }
    }
    if !all && since_ms.is_none() {
        out.truncate(limit);
    }
    remember(
        &out.iter()
            .map(|n| (s(n, &["note_id"]), s(n, &["xsec_token"]), "pc_user".to_string()))
            .collect::<Vec<_>>(),
    );
    Ok((out, more))
}

pub fn hot(p: &mut Page, uid: &str, days: u32, top: usize) -> Result<Vec<Value>> {
    let since = now_ms() - days as i64 * 86_400_000;
    let (mut notes, _) = user_posted(p, uid, 3000, Some(since), false)?;
    notes.sort_by(|a, b| {
        b["likes_n"]
            .as_i64()
            .cmp(&a["likes_n"].as_i64())
            .then(b["time"].as_i64().cmp(&a["time"].as_i64()))
    });
    notes.truncate(top);
    Ok(notes)
}

pub fn note_detail(p: &mut Page, n: &NoteRef) -> Result<Value> {
    let d = p.post(
        "/api/sns/web/v1/feed",
        json!({"sourceNoteId": n.id, "imageFormats": ["jpg", "webp", "avif"], "extra": {"needBodyTopic": "1"}, "xsecSource": n.source, "xsecToken": n.token}),
        false,
        "读取笔记详情",
    )?;
    let items = arr(&d, &["items"]);
    let card = items.first().map(|x| o(x, &["noteCard", "note_card"])).cloned().unwrap_or(Value::Null);
    if card.is_null() {
        return Err(XhsError::new(
            "not_found",
            format!("笔记 {} 没有返回内容（可能已删除、私密或 token 过期）", n.id),
            "换一个新鲜的 xsec_token（重新 `xhs notes` / `xhs search` 拿）再试",
        ));
    }
    let ii = o(&card, &["interactInfo", "interact_info"]);
    let user = o(&card, &["user"]);
    let images: Vec<String> = arr(&card, &["imageList", "image_list"])
        .iter()
        .map(|x| cover_url(x))
        .filter(|x| !x.is_empty())
        .collect();
    let video = {
        let st = o(o(o(&card, &["video"]), &["media"]), &["stream"]);
        let mut u = String::new();
        for k in ["h264", "h265", "av1", "h266"] {
            if let Some(f) = arr(st, &[k]).first() {
                u = s(f, &["masterUrl", "master_url"]);
                if !u.is_empty() {
                    break;
                }
            }
        }
        u
    };
    let tags: Vec<String> = arr(&card, &["tagList", "tag_list"])
        .iter()
        .map(|t| s(t, &["name"]))
        .filter(|x| !x.is_empty())
        .collect();
    let mut t = i(&card, &["time"]);
    if t == 0 {
        t = id_time_ms(&n.id);
    }
    Ok(json!({
        "note_id": n.id,
        "title": s(&card, &["title"]),
        "desc": s(&card, &["desc"]),
        "type": s(&card, &["type"]),
        "time": t,
        "last_update_time": i(&card, &["lastUpdateTime", "last_update_time"]),
        "ip_location": s(&card, &["ipLocation", "ip_location"]),
        "user_id": s(user, &["userId", "user_id"]),
        "nickname": s(user, &["nickname", "nickName"]),
        "likes": s(ii, &["likedCount", "liked_count"]),
        "collects": s(ii, &["collectedCount", "collected_count"]),
        "comments": s(ii, &["commentCount", "comment_count"]),
        "shares": s(ii, &["shareCount", "share_count", "sharedCount"]),
        "liked": b(ii, &["liked"]),
        "collected": b(ii, &["collected"]),
        "images": images,
        "video": video,
        "tags": tags,
        "xsec_token": n.token,
        "url": note_url(&n.id, &n.token, &n.source),
    }))
}

pub fn comment_obj(x: &Value, note_id: &str, parent: &str) -> Value {
    let u = o(x, &["userInfo", "user_info"]);
    let tags: Vec<&Value> = arr(x, &["showTags", "show_tags"]);
    let pics: Vec<String> = arr(x, &["pictures"])
        .iter()
        .map(|p| s(p, &["urlDefault", "url_default", "urlPre", "url"]))
        .filter(|x| !x.is_empty())
        .collect();
    let target = o(x, &["targetComment", "target_comment"]);
    json!({
        "comment_id": s(x, &["id"]),
        "note_id": note_id,
        "parent_id": parent,
        "reply_to": s(target, &["id"]),
        "reply_to_user": s(o(target, &["userInfo", "user_info"]), &["nickname"]),
        "user_id": s(u, &["userId", "user_id"]),
        "nick": s(u, &["nickname"]),
        "avatar": s(u, &["image"]),
        "content": s(x, &["content"]),
        "pictures": pics,
        "create_time": i(x, &["createTime", "create_time"]),
        "ip_location": s(x, &["ipLocation", "ip_location"]),
        "is_author": tags.iter().any(|t| t.as_str() == Some("is_author")),
        "like_count": s(x, &["likeCount", "like_count"]),
        "liked": b(x, &["liked"]),
        "sub_count": i(x, &["subCommentCount", "sub_comment_count"]),
        "at_users": arr(x, &["atUsers", "at_users"]).iter().map(|a| s(a, &["nickname"])).collect::<Vec<_>>(),
    })
}

pub struct CommentOpts {
    pub pages: usize,
    pub sub: bool,
    pub sub_pages: usize,
    pub stop_at: Option<String>,
}

pub fn comments(p: &mut Page, n: &NoteRef, opt: &CommentOpts) -> Result<Value> {
    let mut flat: Vec<Value> = vec![];
    let mut cursor = String::new();
    let mut has_more = false;
    let mut pages = 0;
    let mut found = false;
    while pages < opt.pages {
        let d = p.get(
            "/api/sns/web/v2/comment/page",
            json!({"note_id": n.id, "cursor": cursor, "top_comment_id": "", "image_formats": IMG_FMT, "xsec_token": n.token}),
            "读取评论",
        )?;
        pages += 1;
        for x in arr(&d, &["comments"]) {
            let root = s(x, &["id"]);
            flat.push(comment_obj(x, &n.id, ""));
            for y in arr(x, &["subComments", "sub_comments"]) {
                flat.push(comment_obj(y, &n.id, &root));
            }
            if opt.sub {
                let mut sc = s(x, &["subCommentCursor", "sub_comment_cursor"]);
                let mut sm = b(x, &["subCommentHasMore", "sub_comment_has_more"]);
                let mut sp = 0;
                while sm && !sc.is_empty() && sp < opt.sub_pages {
                    let sd = p.get(
                        "/api/sns/web/v2/comment/sub/page",
                        json!({"note_id": n.id, "root_comment_id": root, "num": 10, "cursor": sc, "image_formats": IMG_FMT, "top_comment_id": "", "xsec_token": n.token}),
                        "读取二级评论",
                    )?;
                    sp += 1;
                    for y in arr(&sd, &["comments"]) {
                        flat.push(comment_obj(y, &n.id, &root));
                    }
                    sc = s(&sd, &["cursor"]);
                    sm = b(&sd, &["hasMore", "has_more"]);
                }
            }
        }
        if let Some(t) = &opt.stop_at {
            if flat.iter().any(|c| c["comment_id"].as_str() == Some(t.as_str())) {
                found = true;
            }
        }
        cursor = s(&d, &["cursor"]);
        has_more = b(&d, &["hasMore", "has_more"]);
        if found || !has_more || cursor.is_empty() {
            break;
        }
    }
    let mut seen = std::collections::HashSet::new();
    flat.retain(|c| seen.insert(c["comment_id"].as_str().unwrap_or("").to_string()));
    Ok(json!({"note_id": n.id, "pages": pages, "has_more": has_more, "cursor": cursor, "comments": flat}))
}

fn search_id() -> String {
    let mut n: u128 = ((now_ms() as u128) << 64) + (rand_range(0, 2_147_483_646) as u128);
    let al = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = vec![];
    while n > 0 {
        out.push(al[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

pub fn search(p: &mut Page, kw: &str, pages: usize, sort: &str, note_type: i64) -> Result<(Vec<Value>, bool)> {
    let sid = search_id();
    let mut out: Vec<Value> = vec![];
    let mut seen = std::collections::HashSet::new();
    let mut more = false;
    for page in 1..=pages.max(1) {
        let d = p.call(
            "WEB_AI_SEARCH_NOTES_V2",
            ".post(",
            json!([{"keyword": kw, "page": page, "pageSize": 20, "searchId": sid, "sort": sort, "noteType": note_type, "extFlags": [], "geo": "", "imageFormats": ["jpg", "webp", "avif"]}]),
            false,
            "搜索笔记",
        )?;
        for x in arr(&d, &["items"]) {
            if s(x, &["modelType", "model_type"]) != "note" {
                continue;
            }
            let id = s(x, &["id"]);
            if id.is_empty() || !seen.insert(id.clone()) {
                continue;
            }
            let tok = s(x, &["xsecToken", "xsec_token"]);
            out.push(card_brief(&id, &tok, o(x, &["noteCard", "note_card"]), "pc_search"));
        }
        more = b(&d, &["hasMore", "has_more"]);
        if !more {
            break;
        }
    }
    remember(
        &out.iter()
            .map(|n| (s(n, &["note_id"]), s(n, &["xsec_token"]), "pc_search".to_string()))
            .collect::<Vec<_>>(),
    );
    Ok((out, more))
}

pub fn user_info(p: &mut Page, uid: &str) -> Result<Value> {
    let d = p.get("/api/sns/web/v1/user/otherinfo", json!({"target_user_id": uid}), "读取用户资料")?;
    let bi = o(&d, &["basicInfo", "basic_info"]);
    let mut counts = serde_json::Map::new();
    for x in arr(&d, &["interactions"]) {
        counts.insert(s(x, &["type"]), json!(s(x, &["count"])));
    }
    let tags: Vec<String> = arr(&d, &["tags"]).iter().map(|t| s(t, &["name"])).filter(|x| !x.is_empty()).collect();
    Ok(json!({
        "user_id": uid,
        "nickname": s(bi, &["nickname"]),
        "red_id": s(bi, &["redId", "red_id"]),
        "desc": s(bi, &["desc"]),
        "gender": i(bi, &["gender"]),
        "ip_location": s(bi, &["ipLocation", "ip_location"]),
        "avatar": s(bi, &["imageb", "images"]),
        "follows": counts.get("follows").cloned().unwrap_or(json!("")),
        "fans": counts.get("fans").cloned().unwrap_or(json!("")),
        "interaction": counts.get("interaction").cloned().unwrap_or(json!("")),
        "followed": s(o(&d, &["extraInfo", "extra_info"]), &["fstatus"]),
        "tags": tags,
        "url": format!("https://www.xiaohongshu.com/user/profile/{uid}"),
    }))
}

pub fn feed(p: &mut Page, more: usize) -> Result<Vec<Value>> {
    let body = format!(
        r#"const MORE={};
const app=document.querySelector('#app')&&document.querySelector('#app').__vue_app__;if(!app)return JSON.stringify({{ok:false,reason:'no_store',msg:'vue app'}});
const pinia=app.config.globalProperties.$pinia;const s=pinia&&pinia._s.get('feed');if(!s||typeof s.fetchFeeds!=='function')return JSON.stringify({{ok:false,reason:'no_store',msg:'feed'}});
const list=()=>(s.feeds||[]).filter(x=>x&&x.id&&(x.noteCard||x.note_card));
if(!list().length){{try{{await s.fetchFeeds()}}catch(e){{return JSON.stringify(apiErr(e))}}await sleep(500)}}
for(let k=0;k<MORE;k++){{await sleep(1500+Math.random()*1800);try{{await s.fetchFeeds()}}catch(e){{return JSON.stringify(apiErr(e))}}await sleep(300)}}
return JSON.stringify({{ok:true,data:list().map(x=>({{id:x.id,xsecToken:x.xsecToken||x.xsec_token||'',noteCard:x.noteCard||x.note_card}}))}});"#,
        more
    );
    let v = p.guarded(false, "读取首页推荐", &body)?;
    let mut out = vec![];
    for x in arr(&v, &["data"]) {
        let id = s(x, &["id"]);
        let tok = s(x, &["xsecToken"]);
        out.push(card_brief(&id, &tok, o(x, &["noteCard"]), "pc_feed"));
    }
    remember(
        &out.iter()
            .map(|n| (s(n, &["note_id"]), s(n, &["xsec_token"]), "pc_feed".to_string()))
            .collect::<Vec<_>>(),
    );
    Ok(out)
}

pub fn liked_or_saved(p: &mut Page, uid: &str, saved: bool, pages: usize) -> Result<(Vec<Value>, bool)> {
    let (path, src, what) = if saved {
        ("/api/sns/web/v2/note/collect/page", "pc_collect", "读取收藏列表")
    } else {
        ("/api/sns/web/v1/note/like/page", "pc_like", "读取点赞列表")
    };
    let mut out = vec![];
    let mut cursor = String::new();
    let mut more = false;
    for _ in 0..pages.max(1) {
        let d = p.get(
            path,
            json!({"num": 30, "cursor": cursor, "user_id": uid, "image_formats": IMG_FMT, "xsec_token": "", "xsec_source": "pc_user"}),
            what,
        )?;
        for n in arr(&d, &["notes"]) {
            let id = s(n, &["noteId", "note_id"]);
            let tok = s(n, &["xsecToken", "xsec_token"]);
            out.push(card_brief(&id, &tok, n, src));
        }
        more = b(&d, &["hasMore", "has_more"]);
        cursor = s(&d, &["cursor"]);
        if !more || cursor.is_empty() {
            break;
        }
    }
    remember(
        &out.iter()
            .map(|n| (s(n, &["note_id"]), s(n, &["xsec_token"]), src.to_string()))
            .collect::<Vec<_>>(),
    );
    Ok((out, more))
}

pub fn unread(p: &mut Page) -> Result<Value> {
    let d = p.call("/api/sns/web/unread_count", "", json!([]), false, "读取未读数")?;
    Ok(json!({
        "unread": i(&d, &["unreadCount", "unread_count"]),
        "mentions": i(&d, &["mentions"]),
        "likes": i(&d, &["likes"]),
        "connections": i(&d, &["connections"]),
    }))
}

pub fn notifications(p: &mut Page, kind: &str, num: usize, pages: usize) -> Result<(Vec<Value>, bool)> {
    let path = format!("/api/sns/web/v1/you/{kind}");
    let mut out = vec![];
    let mut cursor = String::new();
    let mut more = false;
    for _ in 0..pages.max(1) {
        let d = p.call(&path, "", json!([num, cursor]), false, "读取通知")?;
        for m in arr(&d, &["messageList", "message_list"]) {
            let u = o(m, &["userInfo", "user_info"]);
            let item = o(m, &["itemInfo", "item_info"]);
            let ci = o(m, &["commentInfo", "comment_info"]);
            let note_id = if s(item, &["type"]) == "note_info" { s(item, &["id"]) } else { String::new() };
            out.push(json!({
                "id": s(m, &["id"]),
                "type": s(m, &["type"]),
                "title": s(m, &["title"]),
                "time": i(m, &["time"]) * if i(m, &["time"]) < 100_000_000_000 { 1000 } else { 1 },
                "user_id": s(u, &["userid", "userId", "user_id"]),
                "nickname": s(u, &["nickname"]),
                "avatar": s(u, &["image", "avatar"]),
                "note_id": note_id,
                "note_cover": s(item, &["image"]),
                "note_title": s(item, &["content"]),
                "comment_id": s(ci, &["id"]),
                "comment": s(ci, &["content"]),
                "target_comment": s(o(ci, &["targetComment", "target_comment"]), &["content"]),
                "xsec_token": s(u, &["xsecToken", "xsec_token"]),
                "note_xsec_token": s(item, &["xsecToken", "xsec_token"]),
                "note_link": s(item, &["link"]),
            }));
        }
        more = b(&d, &["hasMore", "has_more"]);
        cursor = s(&d, &["cursor"]);
        if !more || cursor.is_empty() {
            break;
        }
    }
    Ok((out, more))
}

pub fn comment_post(p: &mut Page, note_id: &str, text: &str, reply_to: Option<&str>) -> Result<Value> {
    let mut body = json!({"noteId": note_id, "content": text, "atUsers": []});
    if let Some(r) = reply_to {
        body["targetCommentId"] = json!(r);
    }
    let d = p.post("/api/sns/web/v1/comment/post", body, true, "发评论")?;
    let c = o(&d, &["comment"]);
    Ok(
        json!({"note_id": note_id, "comment_id": s(c, &["id"]), "content": s(c, &["content"]), "create_time": i(c, &["createTime", "create_time"]), "reply_to": reply_to.unwrap_or("")}),
    )
}

pub fn write_simple(p: &mut Page, path: &str, body: Value, what: &str) -> Result<Value> {
    let d = p.post(path, body, true, what)?;
    if let Some(c) = d.get("code").and_then(|c| c.as_i64()) {
        if c != 0 {
            return Err(crate::error::classify_api(&p.pkey, Some(c), None, &s(&d, &["msg"]), what));
        }
    }
    Ok(d)
}

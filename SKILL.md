---
name: xhs
description: "小红书（xiaohongshu / RedNote）命令行：在用户已登录的 Chrome 页面里调用页面自己的接口封装（经 amcu），读笔记、评论、搜索、用户、通知，发/删评论，备份评论图片。Use when 要读或操作小红书账号的笔记、评论、搜索结果、通知。"
---

# xhs

前提：`amcu browser doctor` 正常，目标 Chrome profile 已登录小红书。所有命令加 `--json`，输出 `{"ok":true,"data":…}` 或 `{"ok":false,"error":{"code","message","hint"}}`。

## 先确认账号

```bash
xhs profiles --json                     # 选 label
xhs --profile <label> whoami --json     # data.logged_in 必须为 true
```

## 读

```bash
xhs --profile P notes <user_id> --since 7d --json          # data.notes[].note_id / xsec_token / likes_n / time(毫秒)
xhs --profile P hot <user_id> --days 7 --top 3 --json
xhs --profile P note <note_id|带token链接> --json
xhs --profile P comments <note_id> --pages 3 --sub --json   # data.comments[]：comment_id parent_id content pictures create_time ip_location is_author
xhs --profile P comments <note_id> --since 2h --match 加v,vx,代发 --skip-author --json   # 命中项带 hits
xhs --profile P search <关键词> --pages 1 --sort popular --json
xhs --profile P user <user_id> --json
xhs --profile P feed --json
xhs --profile P liked --json ; xhs --profile P saved --json
xhs --profile P notifications --type mentions --json ; xhs --profile P notifications --unread --json
```

`note`/`comments` 需要 xsec_token：先跑 `notes`/`search`/`feed`（自动缓存），或传完整链接 / `--xsec-token`。

## 写（不加 --yes 只返回 confirm_required 和计划，不发请求）

```bash
xhs --profile P comment post <note_id> "内容" [--reply-to <comment_id>] --yes --json   # data.comment_id
xhs --profile P comment delete <note_id> <comment_id> --yes --json
xhs --profile P comment like|dislike <note_id> <comment_id> --yes
xhs --profile P like|collect <note_id> [--undo] --yes
xhs --profile P follow|unfollow <user_id> --yes
```

写之前先不带 `--yes` 跑一次给用户看计划；点赞、关注、收藏真实内容前必须得到用户明确同意。

## 备份 / 下载

```bash
xhs --profile P backup <note_id> <comment_id> --out DIR --json      # DIR/<comment_id>/comment.json + 图片 + 头像
xhs backup <note_id> <comment_id> --out DIR --from comment.json      # 已有评论 JSON，不发请求
xhs img <url> --out DIR/
```

## 必须遵守

- `error.code` 是 `risk_stop` 或 `locked`：立刻停止对该 profile 的一切操作，告诉用户“不要再试，账号可能已被踢，去 Chrome 重新登录”，用户确认后才 `xhs --profile P unlock`。
- `rate_limited`：停 10–30 分钟，不要重试循环。
- 不要并发跑多个 xhs 打同一 profile；不要把 `--pages` / `--all` 开得过大。
- 页面接口找不到（`no_wrapper`）先 `xhs --profile P close` 再试一次；扩展新接口用 `xhs call <path> --args '<json数组>'`（POST 要 --yes）。

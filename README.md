# xhs — 小红书命令行

用 Rust 写的小红书 CLI。它**不自己算签名、不发裸 HTTP 请求**，而是借你已经登录的 Chrome 页面：通过 [amcu](https://github.com/uoox/amcu) 在页面里执行 JS，从 webpack 模块缓存里按接口路径找到小红书页面自己打包的接口封装函数（如 `getApiSnsWebV2CommentPage`、`postApiSnsWebV1CommentDelete`），直接调用。请求由页面自己的 axios 发出，`x-s` / `x-t` / `x-s-common` / `traceid` 全是页面算的，和你在网页上点出来的请求一样。

为什么这样做：自己算签名（哪怕在真实页面里调 `window._webmsxyw`）会立刻返回 300011 并把账号踢下线；纯 HTTP 方案还要 TLS 指纹伪装和设备指纹 cookie。借页面的封装函数是目前唯一稳的路（思路学自 [jackwener/OpenCLI](https://github.com/jackwener/OpenCLI) 的 xiaohongshu 适配器）。

## 安装

依赖：

- macOS / Linux 上的 Chrome，已在要用的 profile 里登录 xiaohongshu.com
- [amcu](https://github.com/uoox/amcu)（`~/.local/bin/amcu`），并在每个要用的 Chrome profile 里装好 amcu 浏览器扩展；`amcu browser doctor` 能连上即可
- Rust stable

```bash
git clone https://github.com/uoox/xhs && cd xhs
cargo install --path .
ln -sf ~/.cargo/bin/xhs ~/.local/bin/xhs
xhs profiles                 # 看 amcu 连着哪些 Chrome profile
xhs --profile jtf whoami     # 确认该 profile 已登录
```

amcu 不在 `~/.local/bin/amcu` 时用环境变量 `XHS_AMCU` 指定。

## 全局参数

| 参数 | 说明 |
|---|---|
| `--profile <label>` | 用哪个 Chrome profile（amcu 的 label / 目录名 / 显示名），也可设 `XHS_PROFILE`。连着多个 profile 时必填 |
| `--session <名>` | amcu 会话名，默认 `xhs-cli`；每个 profile + 会话复用一个后台标签页 |
| `--json` | 输出 JSON：成功 `{"ok":true,"data":…}`，失败 `{"ok":false,"error":{"code","message","hint","api_code"?}}` |

xhs 会在 amcu 的后台窗口里开一个停在 `https://www.xiaohongshu.com/explore` 的标签页并一直复用（id 记在 `~/.cache/xhs/tabs.json`），不打扰你正在看的页面；`xhs close` 关掉它。

## 命令

### 账号

```bash
xhs profiles                         # 列出 amcu 连着的 Chrome profile（--json 给结构化）
xhs --profile jtf whoami             # 当前登录账号：昵称、user_id、小红书号
xhs --profile jtf unlock             # 风控锁定后，确认已重新登录再解锁
xhs --profile jtf close              # 关掉 xhs 开的标签页
```

### 读

```bash
xhs --profile jtf notes 56693828b8c8b47b4cdfcb9d                # 用户最近 30 篇：时间、点赞、置顶、xsec_token
xhs --profile jtf notes 56693828b8c8b47b4cdfcb9d --since 7d     # 7 天内发布的（置顶不影响翻页终止）
xhs --profile jtf notes https://www.xiaohongshu.com/user/profile/56693828b8c8b47b4cdfcb9d --all
xhs --profile jtf hot 56693828b8c8b47b4cdfcb9d --days 7 --top 3 # 最近 7 天点赞最多的 3 篇
xhs --profile jtf note 6ad8ab3f000000003103451b                 # 详情：标题、正文、图片、视频、互动数、标签、IP 属地
xhs --profile jtf note 'https://www.xiaohongshu.com/explore/6ad8…?xsec_token=…'
xhs --profile jtf comments 6ad8ab3f000000003103451b              # 前 3 页一级评论 + 内嵌的二级评论
xhs --profile jtf comments 6ad8ab3f000000003103451b --pages 5 --sub --sub-pages 2
xhs --profile jtf comments 6ad8ab3f000000003103451b --all --sub  # 全部评论和全部二级评论
xhs --profile jtf comments 6ad8ab3f000000003103451b --since 2h  # 只要 2 小时内的（也收秒/毫秒时间戳、2026-10-27 08:00）
xhs --profile jtf comments 6ad8ab3f000000003103451b --match 加v,vx,代发 --skip-author
xhs --profile jtf comments 6ad8ab3f000000003103451b --match ./keywords.txt   # 每行一个关键词，# 开头为注释
xhs --profile jtf search 925银戒指 --pages 2 --sort popular --type image     # sort: general/time/popular  type: all/video/image
xhs --profile jtf user 68581e05000000001b01ae87                 # 资料：粉丝、关注、获赞藏、IP 属地、简介
xhs --profile jtf feed --more 1                                 # 首页推荐
xhs --profile jtf liked                                         # 自己点赞过的（传 user_id 看别人公开的）
xhs --profile jtf saved --pages 2                               # 自己收藏的
xhs --profile jtf notifications                                 # 评论和@
xhs --profile jtf notifications --type likes                    # 赞和收藏；connections = 新增关注
xhs --profile jtf notifications --unread                        # 只看未读数
```

评论字段：`comment_id note_id parent_id reply_to reply_to_user user_id nick avatar content pictures create_time(毫秒) ip_location is_author like_count liked sub_count at_users`，`--match` 时多一个 `hits`（命中的关键词）。
匹配规则与 shelf 面板一致：内容和昵称做全角转半角、去空白和 `·.。,，、_-—~*|/\` 等分隔符、转小写后做子串匹配。

`note` / `comments` 需要 xsec_token。`notes` / `hot` / `search` / `feed` / `liked` / `saved` 会把拿到的 token 缓存到 `~/.cache/xhs/tokens.json`，之后只传笔记 id 就行；也可以传带 `xsec_token` 的完整链接或 `--xsec-token`。

### 写（全部要 `--yes` 或 `--execute` 才真发，否则只校验参数并打印计划，退出码 2）

```bash
xhs --profile bunny comment post 6ae015bd000000000e00f1a0 "写得真好" --yes
xhs --profile bunny comment post 6ae015bd000000000e00f1a0 "谢谢" --reply-to 6ae0a9fb0000000013004dbe --yes
xhs --profile jtf   comment delete 6ae015bd000000000e00f1a0 6ae0a9fb0000000013004dbe --yes   # 自己的评论，或自己笔记下的任何评论
xhs --profile jtf   comment like    <note_id> <comment_id> --yes
xhs --profile jtf   comment dislike <note_id> <comment_id> --yes
xhs --profile jtf   like    <note_id> [--undo] --yes
xhs --profile jtf   collect <note_id> [--undo] --yes
xhs --profile jtf   follow   <user_id> --yes
xhs --profile jtf   unfollow <user_id> --yes
```

写操作前会先读一次 `whoami`，输出里写明“以谁的身份”执行。

### 备份与下载

```bash
xhs --profile jtf backup <note_id> <comment_id> --out ./backup             # 在线翻评论找到它，落 ./backup/<comment_id>/
xhs backup <note_id> <comment_id> --out ./backup --from comment.json       # 已有评论 JSON（文件或 - 读 stdin），不发请求
xhs img 'http://sns-webpic-qc.xhscdn.com/…' --out ./img/                   # 带 referer 下载，--out 是目录或文件名
```

备份目录里是 `comment.json`（评论全文 + files + backup_at）、`pic_0.webp …`、`avatar.jpg`。图片地址带时效签名，过期要重新拉评论拿新地址。

### 直接调接口（给 agent 扩展用）

```bash
xhs --profile jtf call /api/sns/web/v1/user/otherinfo --args '[{"params":{"target_user_id":"68581e05000000001b01ae87"}}]'
xhs --profile jtf call /api/sns/web/v1/you/mentions --verb any --args '[20,""]'
xhs --profile jtf call /api/sns/web/v1/note/like --args '[{"noteOid":"…"}]' --yes
```

按路径字符串在页面模块里找封装函数并调用。GET 封装收 `{params:{snake_case}}`，POST 封装收 camelCase body（页面拦截器会转 snake_case），返回值已被拦截器 camelCase 化。POST 一律要 `--yes`。

## 风控

- 每个 profile 一把文件锁（`~/.cache/xhs/pace-<profile>.lock`），多个 xhs 进程对同一账号串行。
- 每次请求之间随机间隔：读 1.2–2.8 秒，写 3–6.5 秒（跨进程生效）。
- 连续失败指数退避：2s、4s、8s … 上限 60s，成功一次清零。
- 遇到 **300011 / 300012 / 300015 / 461**（或页面被重定向到 `website-login/error` 且是这些码）立即停止，写入 `~/.cache/xhs/kicked-<profile>.json` 锁定该 profile，之后所有命令直接拒绝并提示：**不要再试，账号可能已被踢，去 Chrome 重新登录**；确认恢复后 `xhs --profile X unlock`。
- 300013（访问频繁）、300017 / 300031（笔记暂时无法浏览）、页面返回空白（HTTP 200 正文为空）按软限制处理：报 `rate_limited`，不锁定，计入退避。
- 少量多次、别并发、别把 `--pages` 开很大。面板之类常驻程序自己也要节流。

## 错误码

| code | 退出码 | 含义 / 下一步 |
|---|---|---|
| `invalid_arg` | 2 | 参数格式不对，看 `--help` |
| `confirm_required` | 2 | 写操作没加 `--yes` |
| `need_xsec_token` | 2 | 缺 xsec_token：先 `notes`/`search`/`feed` 或传完整链接 |
| `not_logged_in` | 3 | 该 profile 没登录或登录失效，去 Chrome 登录 |
| `risk_stop` | 4 | 触发硬风控，已锁定；去 Chrome 重新登录后 `unlock` |
| `locked` | 4 | 之前触发过硬风控，仍在锁定中 |
| `rate_limited` | 5 | 软限制，冷却 10–30 分钟 |
| `not_found` | 6 | 笔记/评论不存在或已删 |
| `ambiguous_profile` / `browser_unavailable` / `amcu_missing` | 7 | amcu 连接问题：`--profile`、`xhs profiles`、`amcu browser doctor` |
| `no_wrapper` | 1 | 页面里找不到接口封装（可能改版）：`xhs close` 后重试 |
| `api_error` / `page_error` / `timeout` / `download_error` / `busy` | 1 | 看 message 和 hint |

## 给 agent 用

- 一律加 `--json`，先看 `ok`，失败读 `error.code` 和 `error.hint`；`risk_stop` / `locked` 必须停下交给人。
- 先 `xhs profiles --json` 选 profile，再 `whoami --json` 确认账号。
- 读别人的笔记评论：`notes <uid> --json` 拿 note_id 和 xsec_token（会缓存），再 `comments <note_id> --json`。
- 写操作先不带 `--yes` 跑一遍看计划，确认后再加 `--yes`。
- 仓库根的 `SKILL.md` 是给 agent 的简明用法。

## 状态文件

`~/.cache/xhs/`（可用 `XHS_HOME` 改）：`tabs.json` 标签页 id、`tokens.json` xsec_token 缓存、`pace-*.json` 节流状态、`kicked-*.json` 风控锁。

## 致谢

参考了 [jackwener/OpenCLI](https://github.com/jackwener/OpenCLI)（页面封装函数发现、风控节流）、[jackwener/xiaohongshu-cli](https://github.com/jackwener/xiaohongshu-cli) 与 [jackwener/xhs-cli](https://github.com/jackwener/xhs-cli)（命令设计、SKILL.md）、[cv-cat/Spider_XHS](https://github.com/cv-cat/Spider_XHS)（接口清单）、[lucasygu/redbook](https://github.com/lucasygu/redbook)、[szupzj18/xiaohongshu-cli](https://github.com/szupzj18/xiaohongshu-cli)、[OpenMedia-Lab/xhs-cli](https://github.com/OpenMedia-Lab/xhs-cli)。未复制其代码。

## 许可

MIT

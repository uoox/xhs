# xhs

小红书 CLI（Rust）。核心约束：**绝不自己算签名、绝不发裸 HTTP 到 edith**。所有接口请求都在用户已登录的 Chrome 页面里，调用页面 webpack 模块缓存中的接口封装函数完成；页面通道只走 `amcu` 子进程（`amcu browser --session S --profile P eval --tab T --js …`）。自算签名会 300011 踢号（2026-10-27 实测）。

## 结构

- `src/main.rs` clap 命令定义、分发、人类可读输出（表格/kv）、`--json` 信封 `{ok,data}` / `{ok:false,error}`
- `src/page.rs` 标签页生命周期（`~/.cache/xhs/tabs.json`）、页面 JS 模板（PRELUDE 检测安全限制页/无 app，CALL 按路径找封装并调用）、`guarded()` 包住锁检查 + 节流 + 风控分类
- `src/pace.rs` 每 profile 文件锁、请求间隔、失败退避、`kicked-<profile>.json` 硬风控锁
- `src/api.rs` 各接口参数与返回规整、xsec_token 缓存（`tokens.json`）
- `src/amcu.rs` 子进程封装（超时、错误码映射）
- `src/error.rs` 错误码、退出码、风控码分类（HARD_RISK 锁定 / SOFT_RISK 退避）
- `src/util.rs` 时间解析、id/链接解析、关键词归一化匹配（与 shelf 面板 xhs-comments 的 norm 一致）、表格宽度
- `src/dl.rs` 图片下载（ureq，带 referer）

## 页面里的事实（2026-10 实测）

- `window.webpackChunkxhs_pc_web.push([[id],{},r=>req=r])` 拿 `__webpack_require__`；封装在模块 25717，名字每次构建会变，只按源码里的带引号路径字符串匹配（`"\/api\/…"`，加引号避免 `note/like` 命中 `note/like/page`）。缓存里找不到再扫 `req.m` 工厂并 require。
- GET 封装 `fn({params:{snake_case}})`；POST 封装 `fn(camelCaseBody)`，拦截器转 snake_case；返回值已 camelCase 化；业务错误是 throw，`e.data.code/msg`。
- 搜索没有按路径命名的封装：`getAiSearchNotesV2` 用别名常量 `"WEB_AI_SEARCH_NOTES_V2"` 调 `so.xiaohongshu.com/api/sns/web/v2/search/notes`，所以 search 的 needle 是这个别名。search store（pinia）状态不可靠，别用。
- 通知封装（模块 90853）是位置参数 `fetchMentions(num,cursor)`，源码里没有 `.get(`，verb 传空串。
- feed 读 pinia `feed` store 的 `feeds`，`--more` 调 `fetchFeeds()`。
- 缺 xsec_token 调 comment/page 会被页面拦截器重定向到 `website-login/error?error_code=300031`，整个标签页报废；所以 note/comments 没 token 时直接拒绝。
- 账号被软限制时 `/explore` 返回 HTTP 200 空正文（stgw），页面没有任何 script；ensure 识别为 `rate_limited`。
- user_posted 返回有 `time`（毫秒）；没有时 note id 前 8 位 hex 是秒级时间戳。

## 约定

- 代码无注释；全部文案中文；只有这一个 CLAUDE.md，给人看的只有 README.md，给 agent 用的是 SKILL.md。不新增解释性 md。
- 写操作必须 `--yes`/`--execute`，未确认时返回 `confirm_required`（退出码 2）且不发任何请求。
- 新命令：先用 `xhs call <path> --args …` 在页面里验证封装和参数，再在 `api.rs` 加规整函数、`main.rs` 加子命令，并同步 README 和 SKILL.md。
- 构建：`~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/cargo build --release`；安装 `cargo install --path .` 后软链 `~/.local/bin/xhs`。

## 验证规矩

- 读命令用 `--profile jtf`（径田风）读朋友号 BunnyGleam `56693828b8c8b47b4cdfcb9d`、CherisJewelry `68581e05000000001b01ae87`。
- 写命令只做一次闭环：`--profile bunny` 在径田风（`69675ebb00000000370059e0`）自己的笔记下发评论，再 `--profile jtf` 删掉。不许用径田风在朋友笔记下评论；点赞/关注/收藏只验证到参数校验 + 未加 `--yes` 拒绝。
- 不开新 Chrome 实例，只用已连着的 profile。验证要少量、间隔开；shelf 面板 xhs-comments 也在用同样的账号读。

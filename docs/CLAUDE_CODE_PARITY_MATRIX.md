# Claude Code 功能对照清单

> 审计时间：2026-11-21（Asia/Shanghai）
> Cometix 对照基线：`966326b`（实现 MCP login/logout 之前的 `fork/master`）
> Claude 基线：本机 `claude --version` = `2.1.329 (Claude Code)`
> 官方参考：`https://code.claude.com/docs/en/cli-reference.md`、`commands.md`、`settings-reference.md`、`changelog.md`
>
> 2026-11-25 续查：目标版本更新为 **2.1.332**，npm 官方 registry 与本机 CLI 已交叉核对；周末提交已合入至 `5ad47ae`。下方原始审计仍以 2.1.329 为基线，不能直接当作 2.1.332 的验收结果。推进顺序、资源限制和比较方法见 [WORK_PLAN.md](WORK_PLAN.md)。原版版本/help/changelog 快照及本轮验证证据保存在 `.tmp/parity-20261125/`。

## 2.1.330–2.1.332 增量检查

下列项目来自官方 changelog，当前作为待验收条目。已有相似代码不自动等于新版行为已完成。

| 版本 | 增量或修复范围 | 本轮状态 |
|---|---|---|
| 2.1.332 | MCP server skills（`server:skill`）、`PreSkillShellCommand` hook 与 inline shell 拒绝 | 未执行；原有 MCP skill producer 缺失仍需关闭 |
| 2.1.332 | FIFO 读取 20 秒截止、hook 无限输出/超长日志截断、长 wildcard/长 @-mention 防冻结 | 未执行；需要有时间与内存上限的异常输入回归 |
| 2.1.332 | compaction 连续失败后的最后重试、输出截断后的短摘要恢复 | 未执行；需真实状态转换用例 |
| 2.1.332 | stdio MCP 输出洪泛、SSE 断流换 endpoint 恢复、task ID 长度 1024 | 未执行；不能以普通连接通过替代 |
| 2.1.332 | 会话损坏行、长历史恢复、`/stats`、多工具启动与 print 内存改进 | 未执行；纳入功能与性能共同负载 |
| 2.1.332 | auth 持久化失败 exit 1、NotebookEdit 类型变更、TaskGet blocker 与任务并发更新 | 未执行；分别记录文件/任务副作用 |
| 2.1.332 | Bedrock/Vertex/Foundry 模型别名、allowlist 与 fallback；Remote Control/IDE/Code Mode 修复 | 未执行；provider 与外部宿主分开验收 |
| 2.1.331 | doctor API connectivity、response compression 开关、plugin optionalDependencies | 未执行；现有 doctor 核心文本报告不覆盖新增 connectivity |
| 2.1.331 | MCP malformed reply 截止、compact XML 摘要、hooks 与短终端布局等修复 | 未执行；逐项展开官方 changelog 用例 |
| 2.1.330 | 官方 changelog 增量 | 快照已保存，逐条映射未完成 |

本表是增量索引，详细覆盖仍须逐条映射 changelog；不代表上列几个分组已覆盖全部发布变更。

## 状态定义

- **已实现**：入口、核心行为、错误边界和持久化/输出契约均已覆盖，并有本仓库测试证据。
- **部分实现**：能解析或有局部运行路径，但缺少官方完整行为、平台集成、错误边界或真实链路。
- **缺失**：当前没有可用实现，或 CLI 明确以 `Unimplemented` 退出。
- **关闭/源缺失**：代码存在，但官方功能需要账户、服务端、GrowthBook、闭源运行时或当前参考源码没有实现；不能标为已完成。
- **不适用**：仅在 Claude Desktop、VS Code、企业网关等外部宿主提供的能力；仍记录接口边界，不把它误算为 CLI 已完成。

“能解析参数”不等于“功能已实现”。每个条目必须通过正常路径、异常路径和真实副作用/输出回读后才能改为“已实现”。

## 总体结论

当前仓库已经有较完整的本地 TUI、基础 Agent loop、文件/命令工具、MCP runtime、权限、hooks、skills、插件加载和部分团队任务实现；但与 `2.1.329` 的一比一行为仍有大块缺口。最明显的缺口不是单个 MCP 命令，而是：

1. 顶层 CLI 仍有大量新参数和子命令未接入。
2. `mcp`、`plugin`、`auth`、后台 agent 管理等 shell 子命令没有完整实现。
3. 新版 bundled skills、dynamic workflows、cloud/Remote Control、background supervisor、worktree/tmux、sandbox policy 仍是局部路径或空实现。
4. settings schema 只有约 91 个 Rust 顶层字段，而官方设置参考已超过 249 个条目；字段数量不是完成度指标，但可以直接证明配置面尚未一比一覆盖。
5. 仓库内仍有明确的 `TODO(parity)`、`TODO(port)`、`not ported`、`generated-stub` 和 safe-stub 代码，必须逐项关闭或明确标注关闭原因。

## 一、顶层 CLI 启动面

### 1.1 已有且需要继续做行为回归

| 参数 | 当前状态 | 代码证据/差异 |
|---|---|---|
| `-v`, `--version` | 部分实现 | `src/cli/dispatch.rs` 可退出，但版本文案和官方版本生命周期尚未对齐 |
| `-h`, `--help` | 部分实现 | `src/cli/config.rs` 是手写帮助，缺少 2.1.329 的完整隐藏/高级参数 |
| `-d`, `--debug`, `--debug-file`, `--debug-to-stderr`, `--verbose` | 部分实现 | 参数和日志入口存在，过滤语义、文件轮转、输出契约需实测 |
| `--bare` | 部分实现 | 写入 `CLAUDE_CODE_SIMPLE`，但需验证官方跳过 hooks/LSP/plugin sync/auto-memory/CLAUDE.md/MCP 等完整集合 |
| `-p`, `--print` | 部分实现 | `src/cli/print.rs` 已有完整骨架，需逐项核对 SDK/headless 行为 |
| `--output-format text/json/stream-json` | 部分实现 | 结构化输出已有，hook/task/partial/subagent 事件仍需一比一回归 |
| `--input-format`, `--json-schema` | 部分实现 | schema 校验和错误退出需要对照官方边界 |
| `--include-partial-messages`, `--replay-user-messages` | 部分实现 | 只覆盖部分 stream-json 路径 |
| `--model`, `--effort`, `--thinking`, `--max-thinking-tokens` | 部分实现 | 本地模型/effort 逻辑较完整，新模型能力和 provider 差异未全验 |
| `--fallback-model` | 部分实现 | 当前单字符串投影；官方支持按顺序的逗号列表和重试策略 |
| `--max-turns`, `--max-budget-usd`, `--task-budget` | 部分实现 | 有预算字段，但需验证子 agent、恢复、超预算终态 |
| `--permission-mode`, `--dangerously-skip-permissions`, `--allowedTools`, `--disallowedTools`, `--tools` | 部分实现 | 权限引擎较完整，CLI 值域、模式循环和 headless prompt 行为需回归 |
| `--permission-prompt-tool` | 部分实现 | stdio bridge 已接入；MCP 连接等待、返回 schema、deny 边界未全验 |
| `--disable-slash-commands` | 部分实现 | 有设置入口，需确认 skills、commands、MCP prompts 是否全部关闭 |
| `--resume`, `--continue`, `--from-pr` | 部分实现 | 本地恢复和 picker 存在；跨目录/后台 session/运行中 attach/PR 过滤不完整 |
| `--system-prompt*` | 部分实现 | 文本和文件已有；system prompt snapshot/dynamic boundary 未实现 |
| `--mcp-config`, `--strict-mcp-config`, `--mcp-debug` | 部分实现 | 启动加载存在，连接等待、managed MCP、错误提示需要回归 |
| `--setting-sources`, `--settings`, `--add-dir` | 部分实现 | JSON/path 入口存在；regular-file/2 MiB/来源合并/目录校验需对齐 |
| `--agents`, `--agent`, `--plugin-dir` | 部分实现 | 本地加载路径有；`--print --agents <file>`、严格校验、zip/目录语义需补齐 |
| `--chrome`, `--no-chrome` | 部分实现 | 只有启动开关和部分 UI，完整 Chrome/browser transport 未闭环 |
| `--session-id`, `--fork-session` | 部分实现 | print 路径有逻辑；交互路径仍在 `unimplemented_reasons()` 中拒绝 |
| `--worktree`, `--tmux` | 缺失 | `src/cli/config.rs` 明确归为 `Unimplemented` |
| `--bg`, `--background` | 缺失 | 只有内部 task 数据结构，没有官方 supervisor/worker 生命周期 |
| `--remote` | 缺失 | 仅作为 remote 标记，不等价于官方 cloud session |

### 1.2 2.1.329 官方已有、Cometix 缺失的参数

以下参数必须从 parser、启动状态、帮助、错误边界和实际行为逐项补齐：

| 参数 | 官方行为 | 当前状态 |
|---|---|---|
| `--advisor <model>` | 启用 advisor 模型 | 已实现：解析 `--advisor`/`--advisor=` 并 trim/忽略空值，启动时投影到 `AppState.advisor_model`，优先于 settings/experiment 来源；有效性校验仍由 `/advisor` 命令的 `is_valid_advisor_model` 承担。有 parser 与投影单测；真实 advisor 查询链未在机验收 |
| `--allow-dangerously-skip-permissions` | 将 bypassPermissions 加入交互模式循环，但不立即启用 | 已实现：解析为独立 flag，启动时经 `set_bypass_permissions_mode_available_from_cli` 仅设置 `is_bypass_permissions_mode_available`，不切换初始 mode；遵守组织/设置禁用门。有 parser 与 availability 单测；Shift+Tab 循环行为由既有 `get_next_permission_mode` 覆盖 |
| `--append-subagent-system-prompt` | 为所有子 agent 追加 prompt | 缺失 |
| `--append-subagent-system-prompt-file` | 从文件追加子 agent prompt | 缺失 |
| `--autocompact <auto\|tokens>` | 设置本次会话 auto-compact window | 部分验证（2.1.332 候选）：共享纯解析器取消 1M 上限，接受分数 k/M、数字前缀与观察到的千分分隔，拒绝超出 JS safe integer 的值；`auto` 清除继承覆盖。47 个纯模块测试通过，其中重放 92 条原版接受/拒绝样本；真实 CLI/环境副作用待完整构建，边缘值最终 token 数未全验。见 [CLI_MCP_ACCEPTANCE.md](CLI_MCP_ACCEPTANCE.md) |
| `--ax-screen-reader` | 无边框、无动画的屏幕阅读器输出 | 已实现：解析后在 dispatch 设置 `CLAUDE_CODE_ACCESSIBILITY=1`，复用既有 TextInput 无障碍渲染门禁。有 parser 与 dispatch 单测；完整扁平化/去动画渲染仍由现有 accessibility 渲染路径承担，未做像素级验收 |
| `--betas <betas...>` | API key 请求 beta headers | 已实现：variadic 解析（含 `--betas=` 形式）→ `ANTHROPIC_BETAS` 逗号拼接 + 缓存失效；`utils/betas.rs` 的 `get_all_model_betas` 已读取该变量。有 parser 与 dispatch 测试；真实 API 请求头未在本机抓包验收 |
| `--brief` | 启用 SendUserMessage/brief 能力 | 已实现：解析 `--brief` 后在 dispatch 设置 `CLAUDE_CODE_BRIEF=1` 并直接置位 `USER_MSG_OPT_IN`，复用既有 BriefTool/brief system prompt 能力。有 parser 与 dispatch 单测 |
| `--channels <plugin:...>` | 监听 MCP channel notifications | 缺失/关闭 |
| `--cloud [description\|session_id\|url]` | 创建或继续 cloud session | 缺失 |
| `--correlation-id <id>` | SDK/宿主关联标识 | 缺失 |
| `--dangerously-load-development-channels` | 开发 channel 调试开关 | 缺失/内部 |
| `--desktop` | 转到 Claude Desktop | 缺失 |
| `--enable-auto-mode` | 启用 auto mode | 已实现：解析后在 dispatch 置位 `auto_mode_state` 的 CLI opt-in flag，`has_auto_mode_opt_in_any_source()` 与权限启动流程读取该 flag；feature gate 仍决定是否真正可用。有 parser 与 dispatch 单测 |
| `--environment <id>` | 在 self-hosted cloud environment 创建会话 | 缺失 |
| `--exclude-dynamic-system-prompt-sections` | 将机器动态上下文移到首条 user message | 缺失 |
| `--exec` | 后台 shell job 模式 | 缺失 |
| `--file <file_id:path...>` | 启动时下载文件 | 缺失 |
| `--forward-subagent-text` | stream-json 转发子 agent 文本/思考块 | 缺失 |
| `--ide` | 启动时连接唯一 IDE | 缺失 |
| `--include-hook-events` | stream-json 输出 hook 生命周期事件 | 已实现：解析后在 dispatch 调用 `set_all_hook_events_enabled(true)`，`should_emit_hook_event` 据此放行全部已知 hook 事件，复用既有 hook 事件总线与 stream-json 输出。有 parser 与 dispatch 单测 |
| `--metadata key=value` | 给 self-hosted orchestrator 的会话 metadata | 缺失 |
| `-n`, `--name` | 会话显示名、resume 名称和终端标题 | 已实现：解析 `-n`/`--name`/`--name=`/`-nshort`，启动时经 `apply_cli_session_name` 写入持久化 custom title + agent name（resume 可见）与内存标题缓存（终端标题/prompt bar）；空/空白名忽略。有 parser 与 session-name 应用单测 |
| `--permission-prompts host\|none` | headless prompt 的回答者策略 | 已实现：解析并校验 `host`/`none`（大小写敏感，非法值含 `--version` 路径都 exit 1，错误文案与官方一致）。`host` 启用 SDK stdio 权限桥（要求 `--output-format=stream-json`），`none` 走无 resolver 的 deny-all 默认路径。有 parser 与 dispatch 单测 |
| `--plugin-url <url>` | 本次会话下载 zip plugin | 缺失 |
| `--prompt-suggestions [value]` | stream-json 输出预测的下一条 prompt | 部分验证（2.1.332 候选）：大小写不敏感接受 true/false/1/0/yes/no/on/off；可选值消费下一非 flag token，非法值报错，按参数顺序保留 version/help 短路。纯 parser 已验；dispatch 将别名规范化成 true/false，实际进程与生成链未验 |
| `--ref <branch>` | cloud environment 的基准分支 | 缺失 |
| `--remote-control [name]` / `--rc` | 启动 Remote Control | 缺失 |
| `--remote-control-session-name-prefix` | Remote Control 自动会话名前缀 | 缺失 |
| `--restricted` | 限制工具、配置、工作目录和权限模式 | 缺失 |
| `--safe-mode` | 禁用用户自定义但保留 managed policy | 缺失 |
| `--system-prompt-snapshot on\|off` | 按会话复用或重建 system prompt | 缺失 |
| `--teammate-mode` | in-process/auto/tmux/iTerm2 teammate 展示模式 | 部分/值域未对齐 |
| `--teleport [session]` | 恢复 cloud session 到本地 | 缺失 |
| `--tmux=classic` | 传统 tmux 模式 | 部分，当前只识别布尔 flag |
| `--init`, `--init-only`, `--maintenance` | Setup hooks / SessionStart hooks 的启动阶段 | 缺失 |

### 1.3 顶层 shell 子命令

官方 2.1.329 命令：`agents`, `attach`, `auth`, `auto-mode`, `doctor`, `gateway`, `import`, `install`, `logs`, `mcp`, `plugin|plugins`, `purge`, `respawn`, `remote-control`, `rm`, `setup-token`, `stop|kill`, `ultrareview`, `update|upgrade`，以及 `daemon` 的 `logs/run/status/stop`。

当前 parser 虽列出多个名称，只有表中已明确接入的命令会进入实际 handler；其余命令仍会由 dispatch 按未实现处理：

| 命令组 | 必须实现的子命令 | 当前状态 |
|---|---|---|
| `mcp` | `add`, `add-from-claude-desktop`, `add-json`, `get`, `list`, `login`, `logout`, `remove`, `reset-project-choices`, `serve` | `add`、`add-from-claude-desktop`、`add-json`、`get`、`list`、`login`、`logout`、`remove`、`reset-project-choices` 已覆盖核心来源/行为并有真实二进制测试；`login/logout` 的 HTTP OAuth loopback fixture 覆盖 discovery、手工回调、token 持久化、撤销和幂等清理，真实第三方服务、SSE、claude.ai connector、Mac Keychain 尚未在本机验收。`add-from-claude-desktop` 实现了官方 scope 解析、macOS/WSL 路径定位（含 `%USERPROFILE%` 映射与 `/mnt/c/Users` 扫描）、`mcpServers` 读取、重名编号后缀与按 scope 导入；headless CLI 直接全量导入并回报结果，交互式多选由已有 `MCPServerDesktopImportDialog` 覆盖。Mac/WSL 真机导入未在本机验收（原生 Windows 正确返回官方不支持提示）。`serve` 已通过 5/5 真实二进制用例（初始化、真实工具调用、错误恢复、取消/EOF），stdout 仅协议；工具清单仍不等同于原版全部 22 项 |
| `plugin`/`plugins` | `configure`, `details`, `disable`, `enable`, `eval`, `init/new`, `install/i`, `list`, `marketplace`, `prune/autoremove`, `restore-set-aside`, `tag`, `test`, `uninstall/remove`, `update`, `validate` | 交互 UI/loader 存在，顶层 CLI 全部缺失 |
| `auth` | `login`, `logout`, `status` | 交互命令存在，顶层 CLI 缺失 |
| `agents` | `--json`, `--all`, `--cwd`、派发默认参数 | agent/task 数据结构有，agent view/JSON shell 入口缺失 |
| `auto-mode` | `config`, `critique`, `defaults`, `reset` | 已实现：`defaults` 与冻结原版规则逐字节一致（allow 16 / soft_deny 72 / hard_deny 1 / environment 21），`--label` 过滤一致；`config` 只合并 user/flag/policy 来源，支持 `$defaults`、空数组和 `--settings`，与原版逐字节一致；`reset` 只删除 `autoMode`，确认、`-y`、非法 JSON 边界通过。真实二进制 8/8。已知差异：多顶层键文件的 reset 键顺序不同，JSON 语义一致；`critique` 仅验证无自定义规则时不调用模型，真实模型 side query 未验 |
| `doctor` | 安装、settings、Remote Control、OTel 只读诊断 | 顶层 CLI 已接入：复用 `utils/doctor_diagnostic` 与交互 `/doctor` 共享的数据模型，打印安装类型/版本/平台/路径/搜索/自动更新/问题清单文本报告，有真实二进制测试。远程策略、OTel、安装修复等需网络或副作用的项目未覆盖 |
| `gateway` | `--config` 企业 gateway 进程 | 缺失 |
| `import` | `codex/gemini/cursor`, `--dry-run`, `--yes` | 已实现（仅 2.1.332 未开放契约）：`--help`、unavailable 文案、未知选项与 `--` 边界和原版一致，真实二进制 5/5，17/17 命令对比一致，不写 settings。原版 2.1.332 同样不执行导入，因此没有可对齐的导入行为；不宣称集成已可用 |
| `install` | `stable/latest/version`, `--force` | 缺失 |
| `purge` | 项目数据清理、`--dry-run/-y/-i/--all` | 缺失 |
| `remote-control`/`rc` | server mode、命名、恢复 | 缺失 |
| `respawn` | 指定 session 或 `--all` | 缺失 |
| `attach/logs/stop/kill/rm` | 后台 session 生命周期 | 缺失 |
| `setup-token` | 长期 OAuth token 输出 | 缺失 |
| `ultrareview` | cloud multi-agent review、`--json/--post/--timeout` | 入口存在但被 `ultrareview_enabled() -> false` 隐藏，顶层缺失 |
| `daemon` | supervisor `logs/run/status/stop` | 缺失 |

## 二、会话、后台、worktree、cloud

| 能力 | 当前状态 | 主要缺口 |
|---|---|---|
| session id / named session | 部分 | `--name` 已接入并有应用单测；跨项目搜索、运行中 resume/attach 与完整显示名恢复链尚未验收 |
| fork / branch / rewind | 部分 | `commands/fork` 是 generated-stub，checkpoint 与文件回滚链未完整 |
| background session | 缺失 | supervisor、socket、worker、日志、attach、stop、rm、respawn、恢复 |
| agent view | 部分 | in-process UI 有，shell `claude agents --json`、跨 session 管理未闭环 |
| git worktree | 部分 | Enter/ExitWorktree 工具存在，但 CLI `-w`、命名/PR/MR 基准、清理和 hooks 未闭环 |
| tmux/iTerm2 panes | 缺失 | `--tmux`, `--tmux=classic`, teammate display backend |
| cloud session | 缺失 | cloud create/follow-up/environment/metadata/ref |
| teleport | 缺失 | cloud session picker、branch fetch、transcript attach |
| Remote Control | 缺失 | bridge transport、entitlement、server mode、resume after stop |
| Desktop handoff | 部分 | 只有 slash 命令/UI 投影，缺 Desktop 协议 |

代码证据：`src/tasks/remote_agent_task.rs`、`src/utils/feature_flags.rs` 的 Bridge/RemoteControl、`src/tools/exit_worktree_tool/mod.rs`、`src/cli/config.rs`。

## 三、内置 slash commands / bundled skills / workflows

当前 `src/commands/mod.rs` 有本地命令目录和动态 skill/plugin/MCP prompt 合并，属于基础设施已实现；但官方 2.1.329 的 bundled skills、workflows 仍缺很多。

### 3.1 已有但需要行为回归

`/add-dir`, `/agents`, `/btw`, `/chrome`, `/clear`, `/color`, `/compact`, `/config`, `/context`, `/copy`, `/cost`, `/diff`, `/doctor`, `/effort`, `/exit`, `/fast`, `/help`, `/ide`, `/init`, `/keybindings`, `/login`, `/logout`, `/mcp`, `/memory`, `/model`, `/output-style`, `/permissions`, `/plan`, `/plugin`, `/reload-plugins`, `/rename`, `/resume`, `/skills`, `/stats`, `/status`, `/statusline`, `/tasks`, `/terminal-setup`, `/theme`, `/usage`, `/vim`、`/voice` 等存在不同程度实现；其中 `/vim` 在官方已移除，不能简单按旧版本对齐。

### 3.2 官方有、当前缺失或只有占位的 bundled skill/workflow

`/advisor`, `/artifact-capabilities`, `/artifact-diagramming`, `/artifacts`, `/auto-mode-setup`, `/autocompact`, `/autofix-pr`, `/background`, `/batch`, `/code-review`, `/dataviz`, `/deep-research`, `/design`, `/design-login`, `/design-sync`, `/doctor prompt-audit`, `/goal`, `/loop`, `/run`, `/run-skill-generator`, `/schedule`, `/security-review`, `/simplify`, `/slides`, `/skill-doctor`, `/subtask`, `/team-onboarding`, `/teleport`, `/tui`, `/update-config`, `/verify`, `/web-setup`, `/workflow-authoring`, `/workflows` 以及新版 artifact/design/API/plugin-authoring 技能。

当前明确证据：

- `src/utils/feature_flags.rs:419`：MCP skill producer 缺失。
- `src/utils/feature_flags.rs:551`：Workflow tool not ported。
- `src/utils/feature_flags.rs:424`：Experimental skill search unavailable。
- `src/tasks/local_workflow_task.rs`、`src/tasks/monitor_mcp_task.rs`：generated-stub/no-source。
- `src/commands/mod.rs` 的 `ultrareview_enabled()`、`extra_usage_enabled()`、`thinkback_enabled()` 直接返回 `false`。

## 四、MCP

### 4.1 Runtime 已有但必须继续做一比一复核

stdio、SSE、streamable HTTP、WebSocket、OAuth、elicitation、MCP resources/prompts/tools、动态工具池、权限 prompt bridge、managed MCP、allow/deny schema 均已有代码；但需覆盖连接超时、重连、server message、pending approval、OAuth 410、资源/附件、工具列表变化和失败恢复。

### 4.2 缺失

- 顶层 `mcp serve` 已通过真实二进制 5/5：初始化、真实工具 schema、Read→Edit→Write→磁盘回读、未知工具/非法输入后 ping、取消并回收 shell、EOF 回收 shell；stdout 仅协议。`add`、`add-from-claude-desktop`、`get`、`list`、`login`、`logout`、`remove`、`reset-project-choices`、`add-json` 已接入并有真实二进制测试。`get/list/login/logout` 当前对 plugin/managed/claude.ai/dynamic 来源仍需按官方 scope 再核，不能据核心来源测试宣称完全对齐。
- MCP resources 转 skill/command 的 producer（当前 feature flag 读取不到真实 MCP skills）。
- MCP server 登录退出已接入标准 OAuth CLI 路径，HTTP loopback fixture 已验证；真实第三方兼容、SSE、connector、XAA、plugin/managed/claude.ai 来源与企业设置边界仍需继续对照。
- `add-from-claude-desktop` 已接入 scope/平台检测/路径定位/重名后缀逻辑；Mac/WSL 真机 Desktop config 导入尚未在本机验收（原生 Windows 只验证了不支持提示）。
- claude.ai connector OAuth 的完整 CLI flow，以及标准 OAuth 的更多服务端边界（动态注册、OAuth 410、跨平台密钥链）。
- MCP server `serve` 的 stdin/stdout server mode 已通过 5 个真实二进制用例；工具清单暂不等同于原版全部 22 项，依赖交互界面的工具不向 MCP host 广告。
- `mcp_auth_tool` 对 stdio 认证仍是 `Unsupported`，需与官方认证范围明确分支。

代码证据：`src/commands/mcp.rs`、`src/services/mcp/client.rs`、`src/services/mcp/use_manage_mcp_connections.rs`、`src/utils/feature_flags.rs:419`、`src/tools/mcp_auth_tool/mod.rs`。

## 五、插件、marketplace、skills

插件 manifest、marketplace、依赖、命令/skill/hook 加载和交互 UI 已有较多实现；但是官方 shell 管理面全部未接入，且部分 hook/refresh 行为仍是占位：

- CLI 全部 plugin 子命令：`configure/details/disable/enable/eval/init/install/list/marketplace/prune/restore-set-aside/tag/test/uninstall/update/validate`。
- marketplace source 类型、strict allowlist、blocked marketplace、自动更新状态。
- plugin URL/zip session-only loading。
- plugin dependency resolve/prune/restore-set-aside。
- plugin eval、test、tag、validate。
- `reload-plugins --force` 的 MCP prompt-cache invalidation 和 settings sync。
- plugin prompt hook 当前 `unsupported`。
- plugin channel/mods/Code Mode 尚未接入。

代码证据：`src/services/plugins/*`、`src/commands/plugin/*`、`src/commands/reload_plugins/*`、`src/utils/plugins/load_plugin_hooks.rs:568`。

## 六、Hooks、settings、权限与 sandbox

### 6.1 Hooks

Hook event enum 已覆盖 `SessionStart/Setup/SessionEnd/UserPromptSubmit/PreToolUse/PostToolUse/PostToolUseFailure/PermissionRequest/PermissionDenied/Notification/Elicitation/ElicitationResult/CwdChanged/FileChanged/WorktreeCreate/WorktreeRemove/SubagentStart/SubagentStop/Stop/StopFailure/PreCompact/PostCompact/TaskCreated/TaskCompleted/TeammateIdle/InstructionsLoaded/ConfigChange`。

未闭环项：

- async hook registry 的 shell wrapper/sessionEnvironment。
- HTTP/callback/background hook 的完整执行和超时。
- managed hooks only、managed failure warning、trust dialog 来源提示。
- `DirectoryAdded` 等新版事件与实际调用点。
- `PostToolBatch`、Code Mode hook failure policy。
- file watcher 的 cwd env 清理。
- plugin settings sync、hook reload 和事件 stream 输出。

代码证据：`src/services/hooks/*`、`src/utils/hooks/async_hook_registry.rs`、`src/utils/hooks/file_changed_watcher.rs`、`src/commands/reload_plugins/reload_plugins.rs`。

### 6.2 官方设置参考中当前缺失的设置族

官方 `settings-reference.md` 当前列出约 249 个条目；当前 Rust `SettingsJson` 只有约 91 个顶层字段。缺失重点：

- 模型：`availableModelsMatch`, `deniedModels`, `enforceAvailableModels`, `maxEffortLevel`, `modelPicker`, `modelPricing`, `modelSettings`, `promptCacheTtl`, `subagentPromptCacheTtl`, `switchModelsOnFlag`。
- 权限：`permissions.blockReadsOutsideWorkingDirectories`, `blockWritesOutsideWorkingDirectories`, `denyInsteadOfPrompting`, `autoMode.classifyAllShell`。
- Sandbox：filesystem/network/credentials/Unix socket/Mach lookup/allowed domains/denied domains/managed domains/HTTP proxy/SOCKS proxy/TLS terminate 等完整字段。
- 内存/上下文：`autoCompactEnabled`, `autoCompactWindow`, `bashOutputMaxChars`, `fileCheckpointingEnabled`, `idleCompaction`, `skillListingBudgetFraction`, `skillListingMaxDescChars`, `taskOutputMaxChars`。
- TUI/输入：`axScreenReader`, `editorMode`, `emojiCompletionEnabled`, `maxProseWidth`, `showTurnDuration`, `spellcheck`, `terminalProgressBarEnabled`, `theme`, `timeFormat`, `timeZone`, `tui`, `viewMode`, `vimInsertModeRemaps`, `wheelScrollAccelerationEnabled`。
- 插件/技能：`disableBundledSkills`, `disableSkillShellExecution`, `skillOverrides`, `syncClaudeAiSkills`, `syncClaudeAiPlugins`, `disableCommandPluginSources`, `pluginSuggestionMarketplaces`, `prependPlugins`, `appendPlugins`，以及 strict customization 的完整嵌套策略。
- MCP：`allowAllClaudeAiMcps`, `allowClaudeInChromeWithManagedMcp`, `disableClaudeAiConnectors`, `managedMcpServers`。
- agent/session：`crossSessionInbound`, `disableAgentView`, `isolatePeerMachines`, `processWrapper`, `teammateMode`, `worktree.baseRef/bgIsolation/location`。
- remote/desktop/通知：`agentPushNotifEnabled`, `awaySummaryEnabled`, `disableArtifact`, `disableDesktopLocalSessions`, `disableRemoteControl`, `enableArtifact`, `inputNeededNotifEnabled`, `preferredNotifChannel`, `remoteControlAtStartup`, `sshConfigs`, `sshHostAllowlist`, `sshHostCommands`。
- auth/provider：`allowedProviders`, `forceLoginGatewayUrl`, `gatewayInternalNetworks`。
- enterprise：`disableSideloadFlags`, `forceRemoteSettingsRefresh`, `managedSourcesBehavior`, `parentSettingsBehavior`, `policyHelper`。
- tools/telemetry/global config：browser/mobile simulator、feedback drafts、desktop cleanup、autoConnectIde、autoInstallIdeExtension、copy behavior、default agent view、diff tool、left arrow agent view、team default model 等。

## 七、Tools / Agents / Tasks / Teams

主工具注册表较宽，但以下功能仍不能标为一比一完成：

- `SleepTool` 未注册：`src/tools/sleep_tool/mod.rs` 明确 TODO。
- `tool_execution.rs:9618` 仍有 registered safe-stub tools，需要逐个拆出名称和真实执行契约。
- Skill message loop、tool override、fork subagent runtime 尚有未 port 说明。
- remote agent task 的 spawn/poll/restore/archive/sidecar/notification 缺失。
- auto-dream fork/onMessage/pill/dialog 缺失。
- workflow/monitor task 为 generated stub。
- teammate routing 的 cron/background 仍 deferred，孤儿任务会被丢弃。
- cross-session UDS/bridge peer addressing 缺失。
- agent team 的 tmux/iTerm2/remote display、permission relay、plan approval、stop/idle 完整状态机需回归。

## 八、Compaction、memory、API/provider

- `context_collapse` store-backed collapse/recovery 是 no-op。
- `reactive_compact`、`snip_projection` 是 generated-stub safe no-op。
- agent memory snapshot 只记录 `pendingSnapshotUpdate`，缺 SnapshotUpdateDialog。
- `/clear` 的 prompt cache break、日期 emit、session ingress 清理未 port。
- `verifyApiKey` 仍为 stub。
- API error 分类、401/403 OAuth 恢复、provider fallback、fast-mode cooldown、subscriber/entitlement 判断仍有 TODO。
- Bedrock/Vertex/Foundry 可配置但第三方 provider 的全部认证、模型、错误、auto mode 和 gateway 行为未完成一比一复核。

## 九、IDE、Chrome、voice、desktop、artifact、channels

- IDE：JetBrains 环境变量和连接链路只有半实现；VS Code/IDE diff、accept、file links、terminal integration 不在当前 CLI 中闭环。
- Chrome：只有启动开关/UI，缺浏览器扩展连接、页面操作、console、审批、computer-use 真实链路。
- Voice：feature gate 存在，runtime voice streaming 未实现。
- Desktop：handoff/desktop session/cleanup/parallel panes/preview 缺失。
- Artifacts/design/slides：当前没有 artifact backend、Design、Slides 和下载/发布链。
- Channels：channel notifications、allowlist、permission relay、managed plugin channels 默认关闭或缺 transport。

## 十、2.1.329 近期变更必须纳入回归

从官方 changelog 对照，至少要补以下新版回归：

- 2.1.329：hooks trust 来源提示、managed hook load warning、`/tasks` 汇总 scheduled tasks/loop/goal/forks、MCP server message、marketplace auto-update status、Code Mode hooks/telemetry、scheduled task resume/stop 语义。
- 2.1.328：MCP hook `onFailure=block`、auto mode 第三方 provider、managed mods、OTel endpoint warning、deferred tool resume、self-hosted metadata/hook label、Code Mode 和 background task 语义。
- 2.1.327：`mcp add-json <name> -` stdin 导入（本仓库已实现）。
- 2.1.321 及之后：background agent view、Remote Control、cloud/teleport、plugin eval、workflow、artifact/design、prompt suggestion、restricted/safe mode 等能力必须按版本记录，不得只看最近一条 changelog。

## 十一、实现顺序与提交门槛

按依赖和可本地验收程度排序：

1. CLI parser/help/dispatch：先补新参数的严格解析、值域和错误边界，避免 typo 被 `starts_with` 静默接受。
2. MCP shell 子命令：已完成 `list/get/remove/add/login/logout/reset-project-choices/add-json/add-from-claude-desktop` 与 `serve` 5/5；每个命令独立测试和提交。
3. Auth/install/import/purge 等本地无云依赖命令（`doctor`、`auto-mode` 已完成）。
4. Plugin shell 管理面和 marketplace 本地操作。
5. session-id/fork/name、worktree/tmux、background supervisor/agent view。
6. hooks/structured stream、settings schema、permission/sandbox 完整字段。
7. bundled skills/workflows/scheduled tasks/goal/channels。
8. cloud/Remote Control/teleport/Desktop/IDE/Chrome/artifacts 等外部服务链。

每个功能必须形成：

1. 失败或未实现的稳定回归用例。
2. 最小实现。
3. 定向测试、相关回归和构建记录。
4. 通过后独立中文 Git 提交并推送到 `fork/master`。
5. 回写本文件状态、测试命令、提交号和未覆盖边界。

因此，本文件是当前“全部缺失内容”的基线清单；后续任何“已完成”都必须以这里的条目和测试证据为准。

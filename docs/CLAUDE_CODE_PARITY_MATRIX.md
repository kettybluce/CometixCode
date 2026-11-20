# Claude Code 功能对照清单

> 审计时间：2026-11-20（Asia/Shanghai）
> Cometix 基线：`169ced718a06cf9a190d9c7cd717519ec6ed33a1`（`fork/master`）
> Claude 基线：本机 `claude --version` = `2.1.329 (Claude Code)`
> 官方参考：`https://code.claude.com/docs/en/cli-reference.md`、`commands.md`、`settings-reference.md`、`changelog.md`

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
| `--advisor <model>` | 启用 advisor 模型 | 缺失 |
| `--allow-dangerously-skip-permissions` | 将 bypassPermissions 加入交互模式循环，但不立即启用 | 缺失 |
| `--append-subagent-system-prompt` | 为所有子 agent 追加 prompt | 缺失 |
| `--append-subagent-system-prompt-file` | 从文件追加子 agent prompt | 缺失 |
| `--autocompact <auto\|tokens>` | 设置本次会话 auto-compact window | 缺失 |
| `--ax-screen-reader` | 无边框、无动画的屏幕阅读器输出 | 缺失 |
| `--betas <betas...>` | API key 请求 beta headers | 缺失 |
| `--brief` | 启用 SendUserMessage/brief 能力 | 缺失 |
| `--channels <plugin:...>` | 监听 MCP channel notifications | 缺失/关闭 |
| `--cloud [description\|session_id\|url]` | 创建或继续 cloud session | 缺失 |
| `--correlation-id <id>` | SDK/宿主关联标识 | 缺失 |
| `--dangerously-load-development-channels` | 开发 channel 调试开关 | 缺失/内部 |
| `--desktop` | 转到 Claude Desktop | 缺失 |
| `--enable-auto-mode` | 启用 auto mode | 缺失 |
| `--environment <id>` | 在 self-hosted cloud environment 创建会话 | 缺失 |
| `--exclude-dynamic-system-prompt-sections` | 将机器动态上下文移到首条 user message | 缺失 |
| `--exec` | 后台 shell job 模式 | 缺失 |
| `--file <file_id:path...>` | 启动时下载文件 | 缺失 |
| `--forward-subagent-text` | stream-json 转发子 agent 文本/思考块 | 缺失 |
| `--ide` | 启动时连接唯一 IDE | 缺失 |
| `--include-hook-events` | stream-json 输出 hook 生命周期事件 | 缺失 |
| `--metadata key=value` | 给 self-hosted orchestrator 的会话 metadata | 缺失 |
| `-n`, `--name` | 会话显示名、resume 名称和终端标题 | 缺失 |
| `--permission-prompts host\|none` | headless prompt 的回答者策略 | 缺失 |
| `--plugin-url <url>` | 本次会话下载 zip plugin | 缺失 |
| `--prompt-suggestions [value]` | stream-json 输出预测的下一条 prompt | 缺失/开关关闭 |
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

当前 parser 虽然列出了很多名称，但除了 `mcp add-json` 都会被 dispatch 归为未实现；因此按实际行为全部标为缺失或部分：

| 命令组 | 必须实现的子命令 | 当前状态 |
|---|---|---|
| `mcp` | `add`, `add-from-claude-desktop`, `add-json`, `get`, `list`, `login`, `logout`, `remove`, `reset-project-choices`, `serve` | 仅 `add-json` 已实现，其余缺失 |
| `plugin`/`plugins` | `configure`, `details`, `disable`, `enable`, `eval`, `init/new`, `install/i`, `list`, `marketplace`, `prune/autoremove`, `restore-set-aside`, `tag`, `test`, `uninstall/remove`, `update`, `validate` | 交互 UI/loader 存在，顶层 CLI 全部缺失 |
| `auth` | `login`, `logout`, `status` | 交互命令存在，顶层 CLI 缺失 |
| `agents` | `--json`, `--all`, `--cwd`、派发默认参数 | agent/task 数据结构有，agent view/JSON shell 入口缺失 |
| `auto-mode` | `config`, `critique`, `defaults`, `reset` | 缺失 |
| `doctor` | 安装、settings、Remote Control、OTel 只读诊断 | 交互 `/doctor` 有，顶层 CLI 缺失 |
| `gateway` | `--config` 企业 gateway 进程 | 缺失 |
| `import` | `codex/gemini/cursor`, `--dry-run`, `--yes` | 缺失 |
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
| session id / named session | 部分 | `--name`、跨项目搜索、运行中 resume/attach、显示名持久化 |
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

- 顶层 `mcp add/get/list/login/logout/remove/reset-project-choices/serve`。
- MCP resources 转 skill/command 的 producer（当前 feature flag 读取不到真实 MCP skills）。
- MCP server health/status 的 shell 输出契约。
- `add-from-claude-desktop` 的 Mac/WSL 导入。
- connector/HTTP/SSE 登录和 logout 的完整 CLI OAuth flow。
- MCP server `serve` 的 stdin/stdout server mode。
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
2. MCP shell 子命令：`list/get/remove/add/login/logout/reset-project-choices/serve`，每个命令独立测试和提交。
3. Auth/doctor/auto-mode/install/import/purge 等本地无云依赖命令。
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

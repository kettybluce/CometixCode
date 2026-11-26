# Claude Code 功能对齐与性能工作计划

> 更新日期：2026-11-26，Asia/Shanghai。按单任务、顺序执行推进。
> 合入基线：`5ad47ae537df40acc75d94b783eaac2991359480`。
> 本轮目标：Claude Code **2.1.332**（2026-11-25 同时核实 npm `latest` 与本机 CLI）。
> 后续重新查询 `latest` 时记录新旧版本及增量，保留已验证版本的证据。

用户目标：合入周末在其他环境开发的代码，持续补齐最新版 Claude Code 功能，随后调优性能，并对原版与复刻版给出可复现的功能、性能比较。

## 当前状态

| 项目 | 状态 | 证据与边界 |
|---|---|---|
| 合入周末提交 | 已完成 | `a233551` 快进至 `5ad47ae`，14 个提交；无本地独有提交、工作区冲突或 stash |
| 上游同步 | 已检查 | `Haleclipse/CometixCode` 的 `master` 仍为 `bad7e08`；个人 fork 领先 23 个提交 |
| 最新原版核定 | 已完成 | 本机 `claude --version` 与 npm 官方 registry `latest` 均为 `2.1.332` |
| 本机编译环境 | 已完成 | Rust/Cargo 1.100.0、just 1.59.0、nextest 0.9.152；按用户要求不修改 shell 启动文件 |
| 当前候选验证 | 通过（P1 参数/MCP、P2 auto-mode/import 子阶段） | 本机 Rust 1.88 完整开发构建退出 0；`mcp_serve` 5/5、`cli_parser_contract` 47/47、`cli_value_contract` 4/4、`doctor_cli` 3/3、`mcp_add_json` 35/35；`auto_mode_cli` 8/8，`defaults`/`config`/`--settings` 与原版逐字节一致；`import_cli` 5/5，17/17 命令对比一致，对齐 2.1.332 的未开放契约。当前开发产物 SHA-256 `63f925baef2c7b016cb733e21019065a9f32005edcc13bcffeddd30b58b91fb1`。见 [CLI_MCP_ACCEPTANCE.md](CLI_MCP_ACCEPTANCE.md) |
| 云端验证 | 阻塞 | 当前 PAT 缺少 `workflow` scope，GitHub 拒绝 workflow 文件推送；普通代码推送已成功。已保留配置模板，没有云端构建或测试结果 |
| 本机资源与比较台架 P0-A | 已完成 | 内核资源回读、并发锁、台架同程序自检与报告保护通过；见 [DEVELOPMENT_BASELINE.md](DEVELOPMENT_BASELINE.md)。完整开发构建已在临时 5G 上限下通过，release 构建仍未执行 |
| 功能整体对齐 | 未完成 | 既有清单基于 2.1.329，大量 CLI、会话、插件与外部集成仍存在缺口 |
| 性能比较与调优 | 未执行 | 先取得同环境 release 基线，再用同一套负载测原版与复刻版 |

## 推进顺序

| 阶段 | 工作内容 | 完成判据 | 状态 |
|---|---|---|---|
| P0 基线 | 保存原版 help、版本、最新 changelog、仓库指纹；建立 Rust 与 nextest 环境；检查合入版本 | 可定位的命令、退出码、测试报告与产物指纹；既有失败独立登记 | 已完成：原版参考、环境、合入检查、完整开发构建和真实测试证据已记录 |
| P1 CLI 与 MCP | 关闭严格解析、值域、优先级、参数遗漏问题；补 `mcp serve`；回归新增 11 个参数、Desktop 导入、doctor | 正常、非法、缺参、重复参数、配置覆盖与实际二进制行为有证据；stdio MCP 有完整初始化/列工具/调用/退出测试 | 参数解析与 stdio MCP 已完成真实二进制验收；剩余值域、配置优先级和运行期副作用待继续 |
| P2 本地管理命令 | `auth`、`auto-mode`、`install/update`、`import`、`purge`，以及 plugin/marketplace 管理面 | 用户可从 shell 完成实际管理流程；副作用隔离、持久化回读与错误边界均覆盖 | 进行中：`doctor`、`auto-mode` 已完成真实二进制验收；`import` 已对齐 2.1.332 的未开放契约（原版不执行导入）；`auth`、`install/update`、`purge` 与 plugin/marketplace 管理面未完成 |
| P3 会话与后台 | name/resume/session-id/fork/rewind、worktree/tmux、后台 supervisor、agent view、attach/logs/stop/rm/respawn | 真会话创建、恢复、停止、重启、文件回滚与多任务状态一致；不以结构体或占位 UI 代替实现 | 未完成 |
| P4 执行与配置 | structured stream、hooks、settings schema、权限/sandbox、tools/agents/tasks/teams | 原版兼容用例与真实工具链通过；安全拒绝与恢复行为逐项验证 | 未完成 |
| P5 上下文与扩展 | compact/memory/API/provider、bundled skills、workflows/schedule/goal | 消除相关 no-op/stub，完成实际执行、持久化、错误恢复和 provider 适配用例 | 未完成 |
| P6 外部集成 | cloud/Remote Control/teleport、Desktop/IDE/Chrome/voice/artifacts/channels | 可本机复现部分完成；依赖原版专属账号、服务或平台的项目明确记录契约、测试前置与未验原因 | 未完成 |
| P7 最新版回归 | 对 2.1.330–2.1.332 changelog 逐项映射；重查 npm 最新版本；执行完整功能矩阵 | 每条目标能力有实测/部分/未执行状态、版本、证据；未验项目不算完成 | 未完成 |
| P8 性能基线与调优 | 发布构建、原版对照、定位热点、逐项优化并重跑同负载 | 输出原始样本、分位数、内存/CPU、功能一致性及优化前后差值 | 未完成 |

P0 与 P1 先完成；每批实现后完成相关验证并回写记录。发现合入版本缺陷先复现、修复并补回归，再继续后续功能。

## 功能比较方法

详细条目沿用 [CLAUDE_CODE_PARITY_MATRIX.md](CLAUDE_CODE_PARITY_MATRIX.md)，逐项补到目标版本。状态区分：代码已接入、本地测试通过、真实链路通过、未执行、依赖环境阻塞、明确不适用。参数数量、提交数量、测试数量不换算为整体完成百分比。

每个用例至少记录：目标原版版本、复刻版 commit 与补丁指纹、前置配置、命令或操作、预期、原版实际结果、复刻版实际结果、退出码/响应/文件副作用及未覆盖边界。凭据只从现有安全来源读取，不写入报告。

原版支持的能力要区分本地可复刻实现与专属服务协议。暂时无法访问专属服务不能以空实现或固定成功响应交付；保留未完成项并推进独立可做的工作。

## 性能比较方法

| 场景 | 主要指标 | 对齐条件 |
|---|---|---|
| `--version`、`--help`、doctor、MCP 配置读取 | 冷启动/热启动耗时，用户/内核 CPU，峰值 RSS | 同机顺序执行，固定隔离 HOME/config/cwd，分别报告有无网络 |
| 无 API 的 TUI 启动、输入与长 transcript 渲染 | 首帧、输入到回显、长会话渲染耗时，CPU/RSS | 固定 PTY 尺寸、输入事件、终端与会话样本 |
| 文件读取、搜索、编辑、Bash、MCP | 执行耗时、吞吐、峰值 RSS、结果一致性 | 同一文件树、rg 版本、命令、MCP 服务与输出大小 |
| print / stream-json / 普通聊天 | 首事件、首 token、总耗时、SSE 开销、token 用量 | 同一模型、endpoint、采样参数；先用确定性本机协议 fixture 测客户端开销，再单列真实模型结果 |
| 会话恢复、上下文压缩与后台任务 | 恢复耗时、稳态 RSS、文件/任务终态一致性 | 同样消息、token 预算、任务数、持久化内容与失败时序 |

每个可重复场景默认 3 次预热、至少 20 个正式样本；报告 median、p95、min/max、样本数、退出码、产物大小与 SHA-256。冷启动说明缓存状态，不把普通进程重启称为操作系统冷缓存。优化后复用相同负载，与原版和优化前版本比较；不能通过跳过功能、裁剪结果或放松权限提高分数。

## 本机运行与证据

- 用户明确要求保留当前电脑性能余量。本机 12 个逻辑 CPU、约 11.5 GiB 内存；开工可用内存约 7.8 GiB，已有其他开发服务。
- 开发命令统一通过 `bash scripts/limited-run.sh <command> [args...]`：用 **单核 CPU 亲和性**约束进程树；cgroup 设置**最大 3 GiB 内存、禁止使用 swap**，nice 15、空闲 I/O 优先级、构建/测试均单并发，并用锁避免同时跑两个任务。另设 `CPUQuota=100%`，但本机没有委派 CPU 控制器，必须以亲和性回读为准。开发构建关闭 debug 信息与增量缓存，不改变 release 配置。可用内存不足 4 GiB 时先延后重任务；构建触顶则调整编译方式，不能擅自提高硬上限。早期低于硬上限的软水位会持续触发回收和停顿，最终将软水位与硬上限均设为 3 GiB，保留同样的机器内存余量。
- 2026-11-26 完整开发构建证明单体主库无法压进 3 GiB：3 GiB 在单态化阶段停滞/被 SIGKILL，4 GiB 因软上限持续回收而极慢，5 GiB 下同一源码与编译参数正常完成。该次通过 `scripts/limited-run.sh` 的 `COMETIX_MEMORY_HIGH` / `COMETIX_MEMORY_MAX` 覆盖临时使用 5 GiB，仍保持单核和禁 swap；脚本默认值仍为 3 GiB。后续 release 构建和性能测试必须单独记录资源峰值，不能把这次例外写成常规上限。
- 原版与复刻版性能比较使用相同资源限额；记录测试时机器负载，不做无上限压测。
- 构建使用 release 产物进行性能比较；debug 产物仅用于开发验证。
- 进程级状态测试使用 cargo-nextest，测试任务顺序执行；不把同进程 `cargo test --lib` 当门禁。
- `just test` 只包含 `--lib`；`tests/doctor_cli.rs`、`tests/mcp_add_json.rs` 等实际二进制测试必须单独执行并报告，防止漏掉集成测试。
- 证据目录：`.tmp/parity-20261125/`，包含 `manifest.json`、`reference/`、`evidence/`、`reports/`；忽略运行产物，不提交凭据与原版分发包。
- 每次修改后只补跑受影响测试；阶段完成时再执行适用的全量与 internal feature 门禁。
- 本次未涉及部署。远端发布、共享环境与账号配置遵循用户既有授权。

## 下一项

P1 参数/MCP 子阶段已完成真实二进制验证并提交；P2 的 `doctor`、`auto-mode` 与 `import` 契约也已完成。下一步在 P2 内优先推进 `auth login/logout/status` 或 `install/update`；`auth` 的 logout/credential 写入仍受既有 `OAUTH_CREDENTIAL_SIDE_EFFECTS_ENABLED` 门禁约束，需先明确可验证的安全边界。P1 剩余值域、重复参数/配置优先级和真实运行期副作用继续保留。云端 GitHub Actions 仍受 PAT `workflow` scope 限制，模板保留但未启动；它不再是本机真实测试的前置。获得 release 产物后再开始原版/复刻版性能比较，当前没有性能提升数据。

## 阶段提交约定（2026-11-25 更新）

用户已授权：每完成一个阶段，完成对应验证后使用中文提交，并推送到 GitHub 的个人 fork。后续开发均在 `feat/claude-2.1.332-parity` 业务分支执行；推送不会自动视为发布或部署。保留失败构建、未执行测试及未覆盖能力的真实状态，不为形成提交而把未完成阶段标成完成。

P0 内先收口资源限额与比较台架；P1 拆为参数解析契约、stdio MCP 工具服务、剩余值域/配置覆盖三批。各批分别记录自己的验证边界，P1 总阶段仍以完整完成判据为准。

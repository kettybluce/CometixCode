# 本机资源限制与比较台架验收（2026-11-25）

阶段：P0-A。范围仅为资源执行器、资源回读和 CLI 性能采样台架。本阶段完成；完整构建基线、CLI/MCP 功能验收与原版性能比较仍未完成。

## 测试结论

| 验收项 | 实际结果 | 证据 |
|---|---|---|
| 单核执行 | 进程及子进程继承单个逻辑 CPU 的亲和性；本机用户 cgroup 未委派 CPU 控制器，不能把 `CPUQuota` 配置成功当作实际限流 | `resource-limits-red-1.json` → `resource-limits-green-2.json` |
| 内存与 swap | 从当前进程所在 cgroup 回读：`memory.high=memory.max=3221225472`、`memory.swap.max=0` | `resource-limits-green-2.json` |
| 优先级与并发 | nice 15、I/O idle；Cargo、nextest、Rust tests、Rayon 均为单并发；嵌套运行被 flock 拒绝 | `resource-limits-green-2.json` |
| 台架端到端自检 | 用同一个原版程序充当两个标签；各 3 次预热、20 个正式样本，退出码/输出/统计均有效 | `benchmark-harness-selfcheck-2.json`；**不是复刻版性能结果** |
| 报告与样本保护 | 既有报告拒绝覆盖且哈希不变；少于 20 个样本拒绝启动；shell/Python 语法检查通过 | `tooling-validation-1.json` |
| 完整开发构建 | 未通过：单体库在当前 3G 限额内进入单态化前后被终止，尚无可运行产物；Rust 1.88 与按需代码生成实验同样未解决 | `build-dev-1`～`build-dev-5`、`build-dev-msrv-1`、`build-dev-lazy-1` 日志及 `.resources` |
| 发布构建和功能/性能比较 | 未执行；不能用台架自检、参数纯函数测试或库检查代替 | 尚无 release 产物 |

原始证据统一位于忽略目录 `.tmp/parity-20261125/evidence/`，不提交原版分发包、用户配置、凭据和运行产物。对照目标冻结为此前实测的 Claude Code 2.1.332；后续 P7 再核目标版本。

可复跑的资源核验：

```bash
bash -n scripts/limited-run.sh
bash scripts/limited-run.sh python3 scripts/check-dev-limits.py
```

取得复刻版 release 后执行真实比较：

```bash
bash scripts/limited-run.sh python3 scripts/compare-cli.py \
  --original /path/to/original/claude \
  --cometix target/release/cometix \
  --output .tmp/parity-20261125/reports/cli-comparison-new-attempt.json
```

采样按程序交替、顺序执行，隔离 HOME/config/cwd，保留每次样本和程序 SHA-256；报告 median/p95、CPU 时间和峰值 RSS。普通进程重启不称为冷缓存实验，离线 CLI 启动不能外推为模型延迟或整套功能性能。

## 缺陷与环境问题

1. **资源执行器缺陷，已修复**：本机 scope 只有 `memory/pids` 控制器，`cpu.max` 不存在；早期执行器仍打印单核限制，但实际亲和性有 12 个 CPU。补 `taskset` 后内核回读只含一个 CPU。此前 rustc 的单线程表现不等于 CPU 配额已生效。
2. **构建选项问题，已定位**：当前编译器移除了 `-Zno-parallel-backend`；替换后的 `--jobs-backend=1` 还需要 `-Zunstable-options`。增加微型编译预检，防止全量依赖编完才暴露选项错误。
3. **完整构建受资源约束，未解决**：关闭 debug/incremental、单后端、减少 LLVM 名称，以及独立 allocator/MIR 实验，均未取得完整构建成功证据。失败的标准构建和实验构建分别保留日志；不提高本机 3G 限额，不把部分检查列为完整构建通过。

## 规则改进

- 修改资源包装器后，实际从子进程读取 cgroup、CPU 亲和性和 I/O 优先级；配置值、启动提示不能自证限制生效。
- 性能台架先做同一程序的端到端自检，再比较不同产品。两个标签相同的自检报告必须标明用途，不能产生提速结论。
- 编译参数先在微型样本预检。完整构建失败后保留原 attempt，根据失败阶段调整方案，停止无差别重试。
- P0-A 与功能阶段分别验收和提交；CLI 参数、MCP 真实调用及 release 性能各自保留未验边界。

## 环境变更与清理

- 继续使用业务分支 `feat/claude-2.1.332-parity`；按用户授权，每批完成相应验证后提交并推送个人 fork。
- 所有受限命令结束后 scope 退出、锁释放；未重启或停止他人服务，未部署。
- 原版探针与采样使用的临时 HOME/cwd 自动清理；本任务生成的 Python 缓存已清理。
- 独立 compiler allocator 仅解包在 `.tmp/` 供实验，没有安装系统软件包或修改 shell 默认环境。
- 未提交的功能代码继续留在工作区，验证完成后单独提交；本阶段提交不包含这些功能改动。

## 2026-11-26 补记：完整开发构建与真实测试

P0-A 当时记录的“完整开发构建未通过”仍是历史事实；本轮在同一源码基线上补充了可完成的构建配置和真实测试结果。

| 项目 | 结果 | 证据 |
|---|---|---|
| 3 GiB 构建 | 失败 | `monomorphization_collector_root_collections` 后停滞，随后被 SIGKILL；3 GiB 硬限额不足以容纳单体主库单态化 |
| 4 GiB 构建 | 未完成 | 软上限低于工作集，内核持续回收；系统态 CPU 约为用户态 4 倍，图遍历数分钟无阶段输出 |
| 5 GiB 构建 | 通过 | Rust 1.88、20m01s、峰值 RSS 5,077,244 KiB、无 swap、退出码 0；`monomorphization_collector_graph_walk` 约 28s |
| 真实 CLI/MCP 测试 | 通过 | `mcp_serve` 5/5、`cli_parser_contract` 47/47、`cli_value_contract` 4/4、`doctor_cli` 3/3、`mcp_add_json` 35/35 |
| auto-mode 真实二进制测试 | 通过 | `tests/auto_mode_cli.rs` 8/8；最终排版后重编译加运行 3m33.26s、峰值 RSS 5,374,880 KiB；此前未排版版本 `--no-run` 构建 3m39.82s、峰值 5,448,160 KiB；均无 swap |
| import + auto-mode 合并真实二进制测试 | 通过 | `import_cli` 5/5、`auto_mode_cli` 8/8；重编译加运行 3m47.14s、峰值 RSS 5,363,012 KiB、无 swap |
| auth 真实二进制测试 | 通过 | `tests/auth_cli.rs` 11/11；原版 2.1.332 与复刻版隔离场景 57/57 的 stdout/stderr/退出码一致。`status/logout` 契约通过，`login` 仅帮助与参数，真实 OAuth 与服务端撤销未开放 |
| 受影响 lib 单测（非门禁） | 未完成（资源） | 5G cgroup 下编译 lib test 到 `MIR_borrow_checking` 后被 SIGKILL，峰值 RSS 5,342,468 KiB、无 swap；按工作计划不把同进程 `cargo test --lib` 当门禁，未提高上限 |

产物：`target/msrv-1.88/debug/cometix`。2026-11-25 首次完整构建的 SHA-256 为 `168bccd784f63f405b999f87a2e3d1dfcafc4fa22497dab97ad594088f8e275c`；2026-11-26 加入 auto-mode/import/auth、排版并重跑真实二进制测试后重新链接，当前 SHA-256 为 `addfc7ab53c686b6770adffbf2f68ad98a19a945df1a4bca8d3fc9208f515ee1`，旧指纹已被覆盖。`scripts/limited-run.sh` 新增 `COMETIX_MEMORY_HIGH`、`COMETIX_MEMORY_MAX`、`COMETIX_MEMORY_SWAP_MAX` 覆盖；默认仍是 3 GiB，本轮只在完整构建命令上临时使用 5 GiB，未改常规约束、未启用 swap。

该补记不改变 release 构建和原版性能比较仍未执行的结论。debug 产物只用于开发验证，不能用于性能排名。

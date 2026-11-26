# CLI 与 MCP 候选验证记录（2026-11-25）

目标原版：Claude Code 2.1.332。源基线：`58afa23c9f5141786ec5ca33b6507c45f966d83e`；CLI/MCP 候选为 `8abbd66bbd373c40bb4d33b916e88ab56fe09b0a`，已推送并回读确认。2026-11-26 补测已取得真实开发二进制并通过 CLI/MCP 集成测试；具体文件 SHA-256 见本轮 `.tmp/parity-20261125/manifest.json`。P1 剩余值域与配置覆盖仍待继续，整体 P1 尚未验收完成。

## 测试结论

| 范围 | 状态 | 结果与边界 |
|---|---|---|
| 生产 parser/config/value parser 直接编译测试 | 通过 | 47 个测试；不链接主库，不启动真实 CLI，不验证运行期配置副作用 |
| autocompact 原版接受/拒绝样本 | 通过（仅解析模块） | 同一测试重放 92 条去重样本：61 接受、31 拒绝；样本不证明所有边缘值最终 token 数与原版一致 |
| 大数与分隔符缺陷红绿重放 | 通过 | 隔离旧生产模块 46 通过、1 失败（safe integer 小数边界），修复后 47/47；保留旧模块与原始失败记录 |
| 主库类型/借用检查 | 通过（实验编译参数） | `cargo check --locked --offline --lib` 退出 0；关闭 debug/incremental、使用独立 allocator/MIR 参数，只证明库检查，不证明可链接二进制或测试执行 |
| 五个集成测试目标类型/借用检查 | 通过（仅编译检查） | 候选 `8abbd66` 上 `cli_value_contract`、`mcp_serve`、`cli_parser_contract`、`doctor_cli`、`mcp_add_json` 的 `cargo check --test` 退出 0；没有执行这些测试 |
| 完整本机二进制开发构建 | 通过（构建期临时 5G 上限） | Rust 1.88 主库构建退出 0，耗时 20m01s，峰值 RSS 5,077,244 KiB，未使用 swap；产物 `target/msrv-1.88/debug/cometix`，SHA-256 `168bccd784f63f405b999f87a2e3d1dfcafc4fa22497dab97ad594088f8e275c`。3 GiB 在单态化阶段停滞/SIGKILL，4 GiB 仍被内存回收拖慢，5 GiB 才完成 |
| 真实 CLI 参数、doctor、Desktop/MCP 配置回归 | 通过 | 真实二进制与生产 parser 模块：`cli_parser_contract` 47/47、`cli_value_contract` 4/4、`doctor_cli` 3/3、`mcp_add_json` 35/35 |
| stdio MCP 实际工具调用 | 通过 | `mcp_serve` 5/5：初始化、真实工具 schema、Read→Edit→Write→磁盘回读、未知工具/非法输入后 ping、取消并回收 shell、EOF 回收 shell；stdout 仅协议 |
| 发布构建与原版性能比较 | 未执行 | 没有复刻版 release 产物，不给出产品提速结论 |

## 2026-11-26 真实二进制补测

构建命令保持单核亲和、nice 15、空闲 I/O 和禁止 swap；为完成单体主库的单态化，本次只在当前命令上临时设置 `COMETIX_MEMORY_HIGH=5G`、`COMETIX_MEMORY_MAX=5G`。`scripts/limited-run.sh` 默认仍为 3 GiB，新增的覆盖只用于这一类经过记录的构建异常，不改变常规开发约束。

构建使用的 Rust 1.88 临时编译参数：

```text
-Zmir-strip-debuginfo=all-locals -Ztime-passes
-Zthreads=1 -Zmir-opt-level=0
-Zno-parallel-backend -Zfewer-names=yes
```

`-Zthreads=1` 将类型检查和借用检查峰值压低约 100 MiB，但单态化图遍历仍需要约 4.2–4.8 GiB。3 GiB 尝试在 `monomorphization_collector_root_collections` 后停滞；4 GiB 尝试在软上限以下持续回收，系统态 CPU 远高于用户态；5 GiB 下 `monomorphization_collector_graph_walk` 约 28 秒完成，随后正常生成目标文件。该结果说明此仓库的完整开发构建在当前单体 crate 结构下无法压进 3 GiB，不能用提高 swap 或并行任务绕过。

复跑命令：

```bash
COMETIX_MEMORY_HIGH=5G COMETIX_MEMORY_MAX=5G \
bash scripts/limited-run.sh bash -c '
  env RUSTC_BOOTSTRAP=cometix_code \
    RUSTC_WORKSPACE_WRAPPER="$PWD/.tmp/parity-20261125/compiler-msrv-threads1.sh" \
    CARGO_TARGET_DIR="$PWD/target/msrv-1.88" \
    cargo +1.88.0 test --locked --offline \
      --test mcp_serve --test cli_value_contract \
      --test cli_parser_contract --test doctor_cli --test mcp_add_json \
      -- --test-threads=1
'
```

实际结果：`mcp_serve` 5/5、`cli_parser_contract` 47/47、`cli_value_contract` 4/4、`doctor_cli` 3/3、`mcp_add_json` 35/35。以上都是真实进程测试；parser 47 项同时直接编译生产 `src/cli/{config,parse,value_parsers}.rs`，不链接主库。

补测同时修复了 MCP 服务的两个真实缺陷：

1. `mcp serve` 没有把自建 runtime 发布为进程生命周期 handle，插件加载后台任务 panic：`plugin loading requires process lifetime runtime`。现在与 `main.rs`、`cli/print.rs` 一样调用 `set_process_runtime_handle`。
2. 当前会话原本使用 `current_thread` runtime，Bash 等同步工具会阻塞唯一的协议循环，取消通知和 ping 无法处理；EOF 又要等待 rmcp 的排水超时。现在改为双 worker 多线程 runtime（进程仍由资源包装器固定单核），把每次工具执行放进独立任务并保留 abort controller，stdio 传输在读到 EOF 时主动取消会话并回收子进程。

剩余边界：本批次完成的是 CLI 参数契约与 stdio MCP 子阶段；P1 的其他值域/配置优先级覆盖仍需继续。MCP 广告的是当前 Cometix 注册表实际工具，不等于原版 22 项完全一致；依赖原版专属账号/服务的能力仍未验证。release 产物和原版性能比较尚未开始。

纯模块回归：

```bash
bash scripts/limited-run.sh bash -c '
  rustc --edition=2024 --test tests/cli_parser_contract.rs \
    -o .tmp/parity-20261125/parser-contract &&
  .tmp/parity-20261125/parser-contract --test-threads=1
'
```

原始日志（忽略目录，不随 Git 上传）：

- `evidence/parser-contract-grouping-red-1.log`：隔离旧解析模块，退出码 101。
- `evidence/parser-contract-green-4.log`：最新生产解析模块，退出码 0，47/47。
- `evidence/library-check-final-1.*`：主库检查退出 0，最大 RSS 3,188,448 KiB，无 swap；之后仅排版了 `cfg(test)` 数组，未改运行期语义。
- `evidence/integration-check-final-1.*`：五个集成目标编译检查退出 0，耗时 2m53.66s，`time` 原样报告最大进程 RSS 3,288,620 KiB、无 swap；cgroup 仍配置为 3 GiB 硬限额。RSS 与内核限额分别保留，限额回读见 `resource-limits-green-2.json`。
- `reference/original-option-contracts*.json` 与 `tests/fixtures/cli/autocompact-2.1.332.tsv`：原版探针及接受/拒绝 fixture。
- `evidence/build-dev-msrv-1.*`：Rust 1.88 构建退出 101，最大 RSS 3,251,000 KiB，未使用 swap。
- `evidence/build-dev-lazy-1.*`：Rust 1.100 按需生成/MIR 实验退出 101，最大 RSS 3,306,436 KiB，未使用 swap。

GitHub 拒绝首次推送中的 workflow 文件：当前 PAT 缺少 `workflow` scope。该拒绝不代表构建或产品失败，云端测试没有启动。配置现保留为普通模板；代码推送与云端验收分别记账。

GitHub 托管验证候选使用 `docs/ci/cli-parity.workflow.yml`（启用时复制到 `.github/workflows/cli-parity.yml`），只在功能分支 push 或手动触发时执行；Cargo 与测试均单并发。`scripts/ci-cli-tests.sh` 显式包含 `tests/*.rs` 集成目标，保留 nextest 的真实退出码，上传提交指纹、日志、资源记录和已完成的开发二进制。云端通过之后仍需回读实际运行记录与产物 SHA；提交或推送本身不算通过。该任务不发布、不部署，也不把 debug 产物用于 release 性能对比。

台架静态检查已检查 workflow YAML 和 shell/Python 语法；最初的隔离自检仅覆盖 0/42 两个受控退出码，随后补出并修复空目标、日志写入失败两处误报成功。`python3 scripts/check-ci-harness.py` 现覆盖成功、测试失败、空目标、日志失败、两者同时失败共五个场景，目标枚举、退出码、产物哈希及临时目录清理均通过。这不是产品测试。`mcp.rs` 已通过 rustfmt 检查；`auto_compact.rs` 仍保留继承的格式差异，不做整文件重排。

## 缺陷与归因

1. CLI 值参数曾用前缀匹配，误吞更长的未知参数；现限制为精确名或 `=value`。
2. prompt suggestions 的别名、非法值、可选值消费与原版不同；现按观察到的布尔别名校验，并保留参数顺序短路规则。
3. autocompact 的旧 1M 上限、分数字面量、数字前缀和 safe integer 小数边界有差异；现由共享纯解析器提供 CLI 与运行期服务的同一实现。
4. 多个公开必填选项末尾缺参会静默落入启动路径；现记录缺参并在分发前拒绝。
5. `auto` 原来没有清除继承的窗口覆盖；已补清理代码与 dispatch 用例，实际进程副作用尚待执行。
6. 3 GiB/4 GiB 构建失败已归因于单体主库单态化工作集和软上限内存回收，不是源码编译错误；5 GiB 下同一源码和编译参数退出 0。该修复不改变 release 构建与性能比较仍未执行的结论。
7. CI 台架补测发现 Bash 在无匹配项时会保留 `tests/*.rs` 字面量，以及 `tee` 失败会被只取测试退出码的逻辑掩盖。修复分别启用 `nullglob`、保存管道各段状态并优先保留测试失败；红例见 `ci-script-edges-red-1.json`，修复后五个场景见 `ci-script-edges-green-1.json`。
8. `mcp serve` 的第一版候选缺少进程生命周期 runtime 发布，插件加载后台任务会 panic；同时 `current_thread` runtime 被同步 Bash 工具阻塞，取消通知和 EOF 无法及时处理。已改为双 worker runtime、独立工具任务加 abort controller，并在 stdio EOF 时主动取消会话；`mcp_serve` 5/5 复测通过。

MCP 候选复用现有工具注册表、schema 校验和执行实现，共享文件读取状态，串行执行工具；stdout 仅供协议。它提供当前 Cometix 注册表中的实际工具，不宣称与原版 22 项工具完全一致，不启动模型对话。依赖交互界面的工具不向 MCP host 广告。

## 规则改进与剩余验证

- 版本参数会短路后续选项；未知参数后加 `--version` 的退出码不能证明未知参数拒绝。
- 纯模块测试、库检查、完整构建、真实进程测试分别报告，不能互相替代。
- 验证脚本同时覆盖“没有目标”“日志持久化失败”和“测试与日志都失败”，防止台架把证据缺失记录为成功。
- 取消/EOF 用例以自建 shell 的 PID 和 `/proc` start time 对账、清理；禁止按进程名称误杀其他任务。
- 下次先核云端权限、源提交与运行状态，再做真实 CLI/MCP 验收；任何修复后补跑对应失败场景和受影响回归。
- 后续 P1 仍包括剩余值域、配置优先级和真实运行期副作用；上述候选不能代替整体功能完成。

## 环境变更与清理

常规开发仍默认单核、nice 15、空闲 I/O 优先级、3 GiB cgroup 内存上限与禁用 swap。`scripts/limited-run.sh` 新增 `COMETIX_MEMORY_HIGH` / `COMETIX_MEMORY_MAX` / `COMETIX_MEMORY_SWAP_MAX` 覆盖，用于记录类似本次完整开发构建的例外；默认值未变，本次临时使用 5 GiB 且未使用 swap。新增 Rust 1.88 独立工具链未改变默认 1.100。编译 allocator 只在忽略目录解包，没有修改系统安装或 shell 启动配置。测试使用隔离目录；原始证据和 `target/msrv-1.88` 产物留在忽略目录供复核。未启动子 agent、未停止他人服务、未部署。每个实际完成的阶段使用中文提交并推送个人 fork，未验候选明确标注。

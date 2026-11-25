# CLI 与 MCP 候选验证记录（2026-11-25）

目标原版：Claude Code 2.1.332。源基线：`58afa23c9f5141786ec5ca33b6507c45f966d83e`；本文件描述其后的 CLI/MCP 候选改动，具体提交与文件 SHA-256 见本轮 `.tmp/parity-20261125/manifest.json`。整体 P1 尚未验收完成。

## 测试结论

| 范围 | 状态 | 结果与边界 |
|---|---|---|
| 生产 parser/config/value parser 直接编译测试 | 通过 | 47 个测试；不链接主库，不启动真实 CLI，不验证运行期配置副作用 |
| autocompact 原版接受/拒绝样本 | 通过（仅解析模块） | 同一测试重放 92 条去重样本：61 接受、31 拒绝；样本不证明所有边缘值最终 token 数与原版一致 |
| 大数与分隔符缺陷红绿重放 | 通过 | 隔离旧生产模块 46 通过、1 失败（safe integer 小数边界），修复后 47/47；保留旧模块与原始失败记录 |
| 主库类型/借用检查 | 通过（实验编译参数） | `cargo check --locked --offline --lib` 退出 0；关闭 debug/incremental、使用独立 allocator/MIR 参数，只证明库检查，不证明可链接二进制或测试执行 |
| 完整本机二进制构建 | 阻塞 | 本机 3 GiB 内存上限下 SIGKILL，Rust 1.88 与 1.100 均未取得产物；不提高上限，不使用 swap |
| 真实 CLI 参数、doctor、Desktop/MCP 配置回归 | 未执行 | 已编写或保留 `cli_value_contract`、`doctor_cli`、`mcp_add_json`，需完整产物 |
| stdio MCP 实际工具调用 | 未执行 | 已接入候选代码及 5 个真实二进制用例；初始化、Read→Edit→Write→回读、错误恢复、取消及 EOF 清理都待执行 |
| 发布构建与原版性能比较 | 未执行 | 没有复刻版 release 产物，不给出产品提速结论 |

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
- `reference/original-option-contracts*.json` 与 `tests/fixtures/cli/autocompact-2.1.332.tsv`：原版探针及接受/拒绝 fixture。
- `evidence/build-dev-msrv-1.*`：Rust 1.88 构建退出 101，最大 RSS 3,251,000 KiB，未使用 swap。
- `evidence/build-dev-lazy-1.*`：Rust 1.100 按需生成/MIR 实验退出 101，最大 RSS 3,306,436 KiB，未使用 swap。

GitHub 拒绝首次推送中的 workflow 文件：当前 PAT 缺少 `workflow` scope。该拒绝不代表构建或产品失败，云端测试没有启动。配置现保留为普通模板；代码推送与云端验收分别记账。

GitHub 托管验证候选使用 `docs/ci/cli-parity.workflow.yml`（启用时复制到 `.github/workflows/cli-parity.yml`），只在功能分支 push 或手动触发时执行；Cargo 与测试均单并发。`scripts/ci-cli-tests.sh` 显式包含 `tests/*.rs` 集成目标，保留 nextest 的真实退出码，上传提交指纹、日志、资源记录和已完成的开发二进制。云端通过之后仍需回读实际运行记录与产物 SHA；提交或推送本身不算通过。该任务不发布、不部署，也不把 debug 产物用于 release 性能对比。

台架静态检查已检查 workflow YAML 和 shell 语法；隔离台架自检验证 0/42 两个受控退出码都被如实返回、记录，且目标枚举与产物哈希记录有效。这不是产品测试。所改文件格式检查中，`mcp.rs` 和 `auto_compact.rs` 存在继承的格式差异；保留原有段落，不做整文件重排。

## 缺陷与归因

1. CLI 值参数曾用前缀匹配，误吞更长的未知参数；现限制为精确名或 `=value`。
2. prompt suggestions 的别名、非法值、可选值消费与原版不同；现按观察到的布尔别名校验，并保留参数顺序短路规则。
3. autocompact 的旧 1M 上限、分数字面量、数字前缀和 safe integer 小数边界有差异；现由共享纯解析器提供 CLI 与运行期服务的同一实现。
4. 多个公开必填选项末尾缺参会静默落入启动路径；现记录缺参并在分发前拒绝。
5. `auto` 原来没有清除继承的窗口覆盖；已补清理代码与 dispatch 用例，实际进程副作用尚待执行。
6. 构建失败当前归因为本机资源限制；未取得 MCP 集成测试结果，不能据此判定 MCP 通过或失败。

MCP 候选复用现有工具注册表、schema 校验和执行实现，共享文件读取状态，串行执行工具；stdout 仅供协议。它提供当前 Cometix 注册表中的实际工具，不宣称与原版 22 项工具完全一致，不启动模型对话。依赖交互界面的工具不向 MCP host 广告。

## 规则改进与剩余验证

- 版本参数会短路后续选项；未知参数后加 `--version` 的退出码不能证明未知参数拒绝。
- 纯模块测试、库检查、完整构建、真实进程测试分别报告，不能互相替代。
- 取消/EOF 用例以自建 shell 的 PID 和 `/proc` start time 对账、清理；禁止按进程名称误杀其他任务。
- 下次先核云端权限、源提交与运行状态，再做真实 CLI/MCP 验收；任何修复后补跑对应失败场景和受影响回归。
- 后续 P1 仍包括剩余值域、配置优先级和真实运行期副作用；上述候选不能代替整体功能完成。

## 环境变更与清理

本机保留单核、nice 15、空闲 I/O 优先级、3 GiB cgroup 内存上限与禁用 swap。新增 Rust 1.88 独立工具链未改变默认 1.100。编译 allocator 只在忽略目录解包，没有修改系统安装或 shell 启动配置。测试使用隔离目录；原始证据留在 `.tmp/` 供复核。未启动子 agent、未停止他人服务、未部署。每个实际完成的阶段使用中文提交并推送个人 fork，未验候选明确标注。

# 用于合并的精简更新

基于仓库 `main` 的 `4cd0e76714312e8f2b98f52d5ff3a9623c948b63`。更新包包含先前的网页多线程、默认全部逻辑 CPU，以及本轮精简后的共有证明流程。

## 应用

将 `prp-certificate-web-multithread-update.zip` 解压到本地仓库根目录并覆盖同名文件。包内直接以 `Cargo.lock`、`crates/`、`web/`、`web-dist/` 等开头，没有额外顶层目录；不包含 `.git/`、编译缓存、私有方案、证书或公开大参数。

可用于上述基线，也可覆盖上一版实验更新。包内特意包含恢复原版的 `crates/flight-core/src/groth16/r1cs.rs`，确保上一版的微优化也被替换。已有 `public/groth16/*.dat` 继续使用。

```sh
npm ci
npm run build
npm run dev
```

网页预编译文件已经更新，无需为运行网页安装 Rust nightly。桌面端需按 README 的依赖说明重新编译：

```sh
cargo build --locked --release --bin flight-ui
```

合并前可用 `git diff --stat` 和 `git diff` 审阅。当前更新包没有推送远程仓库或发布 Release；本地确认后提交到自己的主分支即可按已有 Actions 流程构建、部署。

## 本轮保留的共有计算改动

- `engine.rs`：先准备并检查约束，再使用准备好的数据生成证明。
- `groth16/prover.rs`：约束只求值一次，结果同时用于满足性检查和证明多项式；行求值直接复用 arkworks 的实现。
- `Prepared` 的字段保持模块私有，准备结果消费后进入证明，避免外部调用者绕过检查或修改已经检查的数据。
- 独立约束行和多项式逐项运算使用 Rayon 并行；串行与并行的选择集中在 `for_each_pair`、`first_unsatisfied` 两个内部辅助函数。所有模式使用相同运算，错误始终报告最前面的失败约束。
- 保留矩阵兼容性、证明等价性、非法见证回归，以及原生／浏览器计时工具。

约束构建、行合并、SHA-256 摘要以及参数解码恢复或保留原实现。`r1cs.rs` 与多线程基线 `8c7a956` 完全相同。本轮不引入手动摘要编码、原地索引合并、自定义正负一算术捷径或新的依赖。

固定矩阵、公开参数、证书格式、随机数生成和最终证明验证保持兼容。每个矩阵求值后释放，FFT/MSM 前释放全部稀疏矩阵；MSM 仍使用原有的有界分批策略。

## 网页线程策略

默认使用浏览器报告的全部逻辑 CPU 数，至少 1；不减一，不设置 8 线程上限。需要共享内存和跨源隔离；GitHub Pages 由项目内 Service Worker 补充 COOP/COEP。线程启动失败或环境不支持时回退到单线程。任务结束、失败、取消时清理全部计算 Worker。

源码入口是 `web/thread-policy.js` 的 `threadCount()`。普通网页构建校验预编译文件与源码的一致性；修改 Rust 或网页源码后按 README 执行 `npm run build:rust` 重新生成发布文件。

## 验证与性能复现

```sh
cargo test --locked --release -p flight-core
cargo test --locked --release -p flight-core all_circuit_matrices_match_pinned_setup -- --ignored
```

测试覆盖同一随机数下与 arkworks 原证明实现结果一致、非法约束按最前面失败行报告，以及 Q1/Q2/Q3 全尺寸矩阵摘要和统计量与固定参数匹配。完整 Q1 回归还检查非法见证、两端证书生成、浏览器单线程／多线程成绩一致、验证和篡改拒绝，以及取消、回退和线程清理。Q2/Q3 未做完整生成耗时测试，未配置私有参考方案目录。

原生分阶段计时（公开参数从本地 `public/groth16` 读取）：

```sh
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 RAYON_NUM_THREADS=8 cargo run --locked --release -p flight-core --example groth_profile -- /path/to/job.json --prove
```

浏览器完整计时需要 Playwright 和 Chromium，先运行 `npm run build`，然后：

```sh
node scripts/test-web-threads.mjs --job /path/to/job.json --threads 8 --report .cache/benchmark.json
```

`--threads` 只控制测试，不改变应用默认线程数；可用 `--modes parallel` 只测多线程。报告只保存阶段和时间，不保存方案或证书。

本包以明确计算流程、减少重复工作和控制维护成本为目标。上一版实验更新的提速数字不适用于本包，也不能由一次前后运行确定稳定加速比。若评估性能收益，应在同一设备、相同构建配置下交替运行前后版本各 3～5 次，比较中位数，并保持参数来源和线程数一致。

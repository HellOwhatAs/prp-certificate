# 海上油田人员直升机运载计划编排

Rust / egui 网页与跨平台桌面应用。导入航线、人员分配 CSV，在本机检查可行性并生成 Groth16 零知识证明。分享一张 PNG 即可同时展示成绩和离线验证；具体航线与人员分配不写入证书、不上传。

## 使用

三个小问在同一页面分别选择航线表和人员分配表，共用队伍署名。可提交任意一至三问，未提交的小问在证书对应位置留白。文件不完整或校验失败会报错，不能自动当作未提交。

生成前先校验所有已提交方案，再逐问生成证明，最后保存一张三合一 PNG。在“证书记录”中点击“更新”，回填队名和已有成绩，再补充或替换各问方案。原始 CSV 不保存在记录中。每问可分别“校验”或“清空”；修改文件、清空小问或修改队名后，需要重新生成才可导出。其他未修改小问的证明仍可复用。改回原队名可继续复用其证明；新队名需要对应的新证明。生成成功后更新同队名的原记录，改名生成则另存记录，失败或取消不改变原记录。页面底部并排提供“生成证书”和“导出证书”，导出文件为 PNG。仅校验不会生成二维码。第三问人员表列出全部 4000 人，未服务的临时人员安排字段留空。

验证页导入 PNG / JPEG，左侧逐问显示完整成绩指标，右侧显示同一份证书。通过“证书记录”中的“查看”进入时，两侧同步更新；批量导入后展示最近一张，其他已验证证书在记录中查看，导入失败的文件仍单独列出。验证页不另设切换或下载入口。任一问证明无效则整张证书验证失败。记录按整张证书保存，不计算三问总分或统一排名。仅接受 PRG2 三合一格式，不导入旧证书或迁移旧记录。

证书不含具体方案。队名为自填；证明认证方案可行性和成绩，不证明真实身份、算法原创、最优性或可信签发时间。第三问不证明相对基础方案的用时变化。

二维码受到严重压缩、缩小或遮挡时可能无法读取，建议分享原图。验证认证二维码中的成绩，不保证周围可见文字没有被编辑；以导入后显示的成绩为准。

## GitHub 部署与下载

完整步骤见 [DEPLOY_GITHUB.md](DEPLOY_GITHUB.md)。本整理版以 GitHub Pages + 公开 GitHub 仓库为默认部署方式，无需 R2、服务器、密钥或额外服务。

- `main` 推送后由 `.github/workflows/pages.yml` 发布 Pages。
- 推送 `v*` 标签，或在 Actions 中输入已有标签，由 `.github/workflows/release.yml` 编译并发布六个平台：Windows / macOS / Linux × x64 / ARM64。
- 三问参数来自原有固定初始化，SHA-256 与原项目一致；本次未重做参数初始化，未改题目规则或单问证明约束。
- 下载包分为源码、三个参数 ZIP、一个可选的预构建 Pages ZIP。源码包和三个参数包合并后才是可完整部署的项目。

网页 JavaScript 仅负责 WASM 装载、公开证明组件的下载缓存和可取消 Worker；CSV 方案、证书与计算任务均在本机处理。Pages 构建只包含约 30 MB 的静态文件。参数从同一公开仓库的 `raw.githubusercontent.com` 读取，Actions 固定到该次提交 SHA；不依赖旧 Sites 站点。用户无需手动管理网页参数。桌面端也会在首次生成相应小问时自动下载并缓存，逐片校验长度和 SHA-256；不在启动时下载全部三问。

三问首次生成约下载 229 / 428 / 456 MB，按 4 MiB 分片校验并缓存；仅校验方案或验证证书不需要下载大参数。网络需能访问 GitHub Raw，首次生成仍受下载带宽影响。网页建议 64 位桌面浏览器、WebGL2 和至少 8 GB 内存。原有性能记录见 VALIDATION.md。

## 本地运行与构建

使用已提供的网页编译文件预览（需要 Node.js 24，不需要重新安装 Rust）：

```sh
npm ci
npm run build
npm run dev
```

打开 `http://localhost:3000`。本地服务从项目中的 `public/groth16/` 读取大参数。

修改 Rust 或网页代码后重新构建（普通引擎和界面使用 stable；多线程引擎单独使用固定 nightly 重建支持原子操作的标准库）：

```sh
rustup target add wasm32-unknown-unknown
rustup toolchain install nightly-2025-11-15 --component rust-src --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.105 --locked
npm run build:rust
```

`web-dist/` 保存编译文件以及源码/产物哈希；普通构建拒绝过期或被修改的预编译文件。修改代码后需把重新生成的 `web-dist/` 一并提交。Windows 使用 `.gitattributes` 固定文本为 LF，避免自动换行转换破坏哈希。

网页生成证明时自动使用共享内存 Rayon 线程池，参数曲线点解码、约束行求值与检查、FFT、多项式逐项运算和分批 MSM 均可并行。计算线程数使用浏览器报告的全部逻辑 CPU 数，至少 1；不再减一、设置 8 线程上限或按内存分档限制。策略与桌面端默认值一致，但浏览器报告值可能与系统可用逻辑 CPU 数不同。校验和导入证书只加载轻量单线程引擎。任务结束、失败或取消时释放整个线程池。

GitHub Pages 使用项目路径内的 `isolation-worker.js` 补充 COOP/COEP 响应头，首次访问可能在界面出现前自动刷新一次。该 Service Worker 不缓存页面、方案或证书；公开参数仍使用原有分片缓存。不支持跨源隔离、共享内存，或线程启动失败时自动回退到单线程。自建服务器也可以直接返回 `Cross-Origin-Opener-Policy: same-origin` 和 `Cross-Origin-Embedder-Policy: require-corp`。

生成 GitHub Pages 静态文件：

```sh
npm run build:pages -- --repository HellOwhatAs/prp-certificate --ref main
```

输出 `dist/client/`。Actions 自动使用仓库名和实际提交 SHA，无需手改仓库名；上面的 `main` 仅适用于本地准备未上传仓库的预构建包。

构建原生桌面程序：

```sh
cargo build --locked --release --bin flight-ui
python scripts/package-desktop.py
```

Windows 输出 ZIP，macOS / Linux 输出 tar.gz。主程序包包含题目、数据、模板、小型固定验证密钥；首次生成相应小问时自动下载参数，后续复用缓存。仅校验和验证证书不联网下载参数。完成的分片以原子写入保存，中断后重试只补齐缺失或损坏的分片。程序目录无需可写；缓存无法保存时，本次仍可使用内存中的下载数据。

缓存位置：Windows `%LOCALAPPDATA%/prp-flight/parameters`，macOS `~/Library/Caches/prp-flight/parameters`，Linux `${XDG_CACHE_HOME:-~/.cache}/prp-flight/parameters`。`FLIGHT_CACHE_DIR` 可指定其他缓存目录；按参数内容哈希隔离缓存，程序升级后参数未变则继续复用。

离线使用时，可将 Release 中 `prp-flight-parameters-qN.zip` 内的 `public` 文件夹合并到可执行文件所在目录；已有本地参数优先使用，仍检查完整性。三问参数跨平台通用；`FLIGHT_HOME` 可另指定本地参数根目录，`RAYON_NUM_THREADS` 可限制线程数。

Release 工作流通过 `FLIGHT_PARAMETER_REPOSITORY` 和 `FLIGHT_PARAMETER_REF` 将下载地址编入程序，并固定为本次发布的仓库和提交 SHA。手动构建默认使用 `HellOwhatAs/prp-certificate/main`，自建发行版可在编译时设置这两个环境变量；仅改变下载来源，不改变内置参数哈希或验证密钥。

参数体积实测见 [docs/PARAMETER_SIZE.md](docs/PARAMETER_SIZE.md)；本版保持原参数文件不变。

本次没有代替用户运行远程 GitHub Actions；六个平台配置需在上传后由实际 runner 执行。未进行 Windows / Apple 开发者签名。

## 证明与固定规则

- `crates/flight-core`：CSV 校验器、原生 R1CS 约束、arkworks Groth16 BN254 生成与固定密钥验证。
- `crates/flight-format`：严格元信息、二进制证书载荷，独立于证明与界面。
- `crates/flight-ui`：egui、素色证书、标准 QR 编解码。
- `public/groth16`：每问固定公开参数分片、验证密钥和 SHA-256 清单。
- `circuits`：迁移前 Noir 规则参考；不参与当前运行或构建。
- `src/checker.js`：独立旧校验器，仅用于开发回归。

固定题目数据编入约束：Q1 1600 人，Q2/Q3 4000 人。约束重算九项整数成绩，检查必运人员、起终点、首次下客、容量、停靠次数、余油、飞行时间、时间窗和飞机周转。最多 256 架次、每架次最多 7 站；更多架次仍可 CSV 校验，但不能生成当前证明。

为避免逐行内存选择产生过大电路，使用确定性的 AS-Waksman 置换网络证明查询表一致性，并按站点累计上下客事件。查询包含索引、路线与时间载荷、人员事件，三者共用同一组布尔交换；相邻相同索引值相等，事件总和与逐站载客量匹配。没有自行引入随机挑战的内存证明。

原生端与网页端共用约束构建、求值和证明实现。约束行使用 arkworks 的求值函数，只求值一次，结果同时用于满足性检查和证明多项式；独立行与多项式逐项运算在支持线程时并行，串行／并行选择集中在内部辅助函数中。已经检查的数据通过字段私有的 `Prepared` 交给证明阶段。每个矩阵求值后释放，全部稀疏矩阵在 FFT/MSM 前释放；多标量乘法仍按每批 65,536 项执行，并及时释放用完的参数向量。小型电路对同一随机数输出与上游逐字节相同的证明，不降低约束或关闭零知识。约束构建、行合并和摘要计算保留直接实现。

公开输入为 32 个身份摘要字节和 9 项 u32 成绩。元信息按 `[format,rules,dataset,question,team,nonce]` 的紧凑 JSON 数组作 SHA-256；题号、队名、随机编号、数据集与规则因此绑定证明。规则为 `prp-flight-final-g16-v1`，协议为 `prp-flight-groth16/1`。证明使用新鲜平台安全随机数，压缩后 128 字节。

验证器只使用编入程序的固定 VK，不接受证书自带密钥。公开参数在反序列化前与程序内置 SHA-256、大小匹配；每批最多 32,768 个点分块读取，原生端与支持多线程的网页端并行还原曲线点。参数编码与固定密钥不变，已下载的参数包继续使用。证明前再检查矩阵摘要和约束满足性。PNG 的 PRG2 格式见 [IMAGE_FORMAT.md](public/IMAGE_FORMAT.md)。

## 初始化信任

本版三个电路分别执行一次 Groth16 可信初始化，使用操作系统 CSPRNG。程序只输出公开 PK/VK，没有输出或持久化初始化秘密、随机种子或陷门标量。初始化进程退出后仍需信任执行环境没有泄露其内存；这不是多方仪式。若初始化秘密泄露，可伪造证明。此信任假设不能用测试消除。

参数清单记录约束矩阵和公开参数摘要。修改任何固定规则、数据或电路结构，都必须重新初始化、升级协议并发布新的验证程序。`groth_setup` 拒绝覆盖已有密钥；常规构建、CI 与运行不执行初始化。项目未通过独立密码学审计，arkworks 和本项目的依赖、约束代码、编译器及发布程序均在信任边界内。

## 验证与开发

```sh
cargo test --locked --workspace --release
cargo test --locked -p flight-core --release all_circuit_matrices_match_pinned_setup -- --ignored
node scripts/test-carrier.mjs
REFERENCE_DIR=/path/to/private/trial_solve cargo test --locked -p flight-core --release --test checker
REFERENCE_DIR=/path/to/private/trial_solve npm test
```

完整证明与对抗回归使用不随源码分发的 `.cache/rust-tests/job-qN.json`。`groth_regression` 直接篡改私密见证，验证约束拒绝不合法方案；`test-certificates.py` 验证元信息、成绩、证明篡改；`test-web-release.mjs` 测试准确构建出的 Worker 与 WASM；`test-web-preflight.mjs` 检查无效方案不会触发参数读取；`test-images.py` 测试压缩、缩放、旋转、裁切、遮挡。

性能复现：`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 RAYON_NUM_THREADS=8 cargo run --locked --release -p flight-core --example groth_profile -- /path/to/job.json --prove` 分别输出方案校验、见证转换、构建、摘要、约束检查和完整生成流程的时间；公开参数从本地 `public/groth16` 读取。浏览器先运行 `npm run build`，再用 `node scripts/test-web-threads.mjs --job /path/to/job.json --threads 8 --report .cache/benchmark.json` 对比固定 8 线程与单线程；可用 `--modes parallel` 只测多线程。线程数参数只控制测试，不改变应用的默认策略。报告不保存方案或证书。前后对比应使用相同方案、线程数、构建配置和设备，避免同时运行其他计算任务。

本轮精简合并说明见 [WEB_THREADS_UPDATE.md](WEB_THREADS_UPDATE.md)。原有发布验收记录见 DELIVERY_CHECKS.md；底层证明的已有性能记录见 VALIDATION.md。Node 仅作为开发测试宿主，不进入应用运行依赖。

字体使用 Noto Sans SC（SIL OFL 1.1）。依赖版本锁定于 Cargo.lock，许可证与置换网络来源见 public/THIRD_PARTY_LICENSES.txt。

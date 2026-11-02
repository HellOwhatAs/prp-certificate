# 手动部署到 GitHub

目标仓库：<https://github.com/HellOwhatAs/prp-certificate>。本包未向远端推送任何文件，也未替你开启 Pages 或发布 Release。

## 1. 下载并合并

| 下载文件 | 内容 | 是否需要 |
| --- | --- | --- |
| `prp-certificate-source.zip` | Rust 源码、预编译 WASM、题目 PDF、数据、模板、GitHub 工作流和本说明 | 必需 |
| `prp-flight-parameters-q1.zip` | 第一问公开证明参数，约 229 MB | 完整部署必需 |
| `prp-flight-parameters-q2.zip` | 第二问公开证明参数，约 428 MB | 完整部署必需 |
| `prp-flight-parameters-q3.zip` | 第三问公开证明参数，约 456 MB | 完整部署必需 |
| `prp-certificate-pages.zip` | 已构建的纯静态网页，便于单独留档或静态托管 | 可选；Actions 会自行生成 |

源码 ZIP 中有一个 `prp-certificate/` 目录。三个参数 ZIP 的顶层都是 `public/`，将它们解压到源码项目根目录并合并文件夹。正确结果是 `prp-certificate/public/groth16/q1-0.dat` 等 267 个 `.dat` 文件，与 `Cargo.toml` 同属一个项目。

**上传解压后的文件，不要把这些 ZIP 原样提交到 GitHub。** 不要使用 Git LFS 管理 `.dat`，本配置使用 GitHub Raw 读取实际文件。不得重新生成 Groth16 参数，否则会使现有证书失效。

## 2. 上传到已有仓库

将仓库设为 **Public**。这个配置需要浏览器匿名访问公开参数。安装 Git 后，先克隆已有仓库，避免覆盖已有历史或出现 unrelated histories：

```sh
git clone https://github.com/HellOwhatAs/prp-certificate.git prp-certificate-github
```

把源码包中 `prp-certificate/` 的**全部内容**复制到 `prp-certificate-github/`，包括 `.github`、`.gitattributes`、`.gitignore`。保留克隆产生的 `.git/`。将三个参数包内的 `public/` 文件夹也合并进去。

Windows PowerShell 的参数解压示例（从下载包所在目录执行）：

```powershell
Expand-Archive .\prp-flight-parameters-q1.zip -DestinationPath .\prp-certificate-github -Force
Expand-Archive .\prp-flight-parameters-q2.zip -DestinationPath .\prp-certificate-github -Force
Expand-Archive .\prp-flight-parameters-q3.zip -DestinationPath .\prp-certificate-github -Force
```

进入项目并提交：

```sh
cd prp-certificate-github
git branch -M main
git add .
git status --short
git commit -m "Add PRP certificate app, Pages and release workflows"
git push -u origin main
```

推荐用 Git 或 GitHub Desktop 上传；网页上传一次只能处理有限文件，容易漏掉工作流或参数分片。完整项目约 1.15 GB，首次推送较慢；其中每个参数分片不超过 4 MiB。不要把 `target/`、`dist/`、`release/` 或下载 ZIP 一起提交。

如果已安装 Python 3，可在提交前检查参数：

```sh
python scripts/test-parameter-integrity.py
```

应打印 Q1、Q2、Q3 全部验证通过。没有 Python 也可直接推送，Pages 工作流会执行同样检查。

## 3. 开启 GitHub Pages

1. 打开仓库 **Settings → Pages**。
2. 在 **Build and deployment → Source** 选择 **GitHub Actions**。
3. 打开 **Actions → Deploy GitHub Pages → Run workflow**，选择 `main` 运行。如果首次推送已成功触发，无需重复运行。
4. 等 `build` 和 `deploy` 都成功，访问 <https://HellOwhatAs.github.io/prp-certificate/>。

若 GitHub 提示工作流被禁用，在仓库 Actions 页启用工作流；不需要创建 PAT、仓库 Secret、Cloudflare 账号或 R2 存储。若首次运行早于 Pages 设置而失败，完成设置后重新运行即可。

网页、CSV 校验、证明生成和验证均在浏览器运行。约 1.11 GB 的公开证明参数保存在 Git 仓库中，由 GitHub Raw 提供；Pages 仅发布约 30 MB 静态文件。每次 Actions 构建将下载地址固定到实际提交 SHA，且每片仍会校验 SHA-256。

`prp-certificate-pages.zip` 的预构建地址是 `HellOwhatAs/prp-certificate/main`，适用于当前目标仓库。走上述 Actions 部署时不必手动上传这个 ZIP；Actions 会生成更严格固定到提交 SHA 的版本。更换仓库名也只需让 Actions 重新构建。

## 4. 编译并发布 GitHub Release

代码和三个参数包全部上传后，创建一个新版本标签：

```sh
git tag v0.3.0
git push origin v0.3.0
```

这会触发 **Actions → Build and publish Release**。以后使用新的版本号，例如 `v0.3.1`；不要覆盖旧标签。

也可在 Actions 手动运行：在 `tag` 输入框填写一个**已经推送到仓库**的标签，例如 `v0.3.0`。工作流会核对标签并让所有构建使用同一提交。

| 系统 | 架构 | Release 附件 |
| --- | --- | --- |
| Windows | x64 | `prp-flight-windows-x64.zip` |
| Windows | ARM64 | `prp-flight-windows-arm64.zip` |
| macOS | Intel x64 | `prp-flight-macos-x64.tar.gz` |
| macOS | Apple Silicon ARM64 | `prp-flight-macos-arm64.tar.gz` |
| Linux | x64 | `prp-flight-linux-x64.tar.gz` |
| Linux | ARM64 | `prp-flight-linux-arm64.tar.gz` |

还会上传三问参数 ZIP、源码 ZIP、Pages ZIP 和 `SHA256SUMS.txt`，共 12 个附件。所有构建和测试成功后才会发布；上传附件期间先保留为草稿，上传成功后转为正式 Release。失败时不会发布一个新建的半成品 Release；修复后可重新运行失败工作流。

原生程序来自 Rust 编译；Pages 使用源码包内经过哈希检查的预编译 WASM。六个平台配置已经提供，远程编译结果需以你上传后 GitHub Actions 的实际运行状态为准。

普通使用者只需下载对应系统的桌面程序。验证证书无需额外参数；生成第 N 问证书时，把对应参数 ZIP 中的 `public/` 合并到程序所在目录即可。三问参数可同时放入同一目录。

## 5. 后续修改

只改部署说明、工作流、打包脚本时，无需重新编译 WASM。修改 `crates/`、网页脚本、题目数据、依赖或构建脚本时，需要重新生成 `web-dist/`：

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.105 --locked
npm ci
npm run build:rust
```

之后提交源码和更新后的 `web-dist/`。不要手工修改 `web-dist/release.json` 绕过过期检查。固定题目数据或约束的变化还涉及证明协议及初始化；普通 UI 修改不需要更换参数。

## 6. 常见问题

- **缺少 qN-*.dat / 参数哈希检查失败**：三个参数 ZIP 没有完整合并，或被 Git LFS 指针替代；检查 `public/groth16/`。
- **网页能开但无法生成证书**：确认仓库公开、参数全部上传，且用户网络能访问 `raw.githubusercontent.com`。无须向页面增加参数管理按钮。
- **网页编译文件与源码不匹配**：按第 5 节重新构建；Windows 保留 `.gitattributes`，避免 CRLF 转换破坏源文件哈希。
- **Pages 404 或 configure-pages 失败**：先在 Settings → Pages 选择 GitHub Actions，再重新运行。部署后以 Actions 给出的地址为准。
- **Release 没出现**：`main` 推送只部署 Pages；发布 Release 需要推送 `v*` 标签或手动选择已有标签。查看是否有某个平台失败，`publish` 会等全部成功。
- **发布权限错误**：工作流的 `publish` 已声明 `contents: write`；若组织策略禁止写权限，需由仓库/组织管理员允许该工作流。
- **macOS / Windows 安全提示**：发行程序未进行 Apple / Windows 开发者签名；下载者应确认来源。Linux 需要 X11/Wayland/OpenGL 桌面运行库，构建基于 Ubuntu 22.04。
- **不要上传临时测试解**：源码包不包含组织者私有参考解、账号凭据、原聊天或旧工作区缓存。

## 官方说明

- [GitHub Pages 限额](https://docs.github.com/en/pages/getting-started-with-github-pages/github-pages-limits)
- [配置 Pages 发布来源](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site)
- [GitHub 托管 runner 和架构](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
- [GitHub 仓库限制](https://docs.github.com/en/repositories/creating-and-managing-repositories/repository-limits)

本次整理日期：2026-11-02；源自提交 `66ac2ea`。原始研究/证明设计及先前验证结果见 README.md 和 VALIDATION.md。

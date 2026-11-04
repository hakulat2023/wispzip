# WispZip（一压）

[English](README.md) | **简体中文**

轻量级文件压缩 / 解压工具 —— **主界面极简，拖拽优先，右键即用，复杂能力全部收进"高级配置"**。
架构采用 **「一套核心 + 多个薄壳」** 模式：全部业务逻辑下沉在 Rust 核心 crate，桌面 App 与
CLI 只是薄壳，共享同一引擎。

- 桌面端：**Tauri 2 + Vue 3 + TypeScript**
- 核心引擎：**纯 Rust**（`wispzip-core`），流式处理，GB 级文件不爆内存

## 下载与安装

预编译产物（dmg / msi / deb / AppImage）见 [Releases](https://github.com/hakulat2023/wispzip/releases)。

**macOS 首次打开必读：** App 未经 Apple 公证（无开发者账号），从网上下载的 dmg / zip 自带隔离属性，
首次打开可能提示 **"已损坏，无法打开"** 或 **"无法验证开发者"**。任选其一解决：

```bash
# 推荐:清除隔离属性后即可正常打开
xattr -cr /Applications/WispZip.app
```

或者:**右键 WispZip.app → 打开**(不要直接双击)。此操作只需一次,
之后打开不再有任何提示。Windows / Linux 产物解压即用,无此问题。

## 功能

| 格式 | 压缩 | 解压 | 备注 |
|---|---|---|---|
| zip | ✅ | ✅ | Deflate 0–9；AES-256 密码；ZIP64；UTF-8/GBK 文件名自动识别 |
| tar | ✅ | ✅ | 保留权限 / mtime |
| tar.gz / tar.bz2 / tar.xz / tar.zst | ✅ | ✅ | 流式管道，进度实时上报 |
| gz / zst / xz / bz2 | ✅ | ✅ | 单文件流压缩（xz 高压缩、bz2 平衡），保留原文件名去后缀 |
| 7z | ✅ | ✅ | LZMA2 0–9；AES-256 密码（内容加密，文件头可无密码列出）；保留 Unix 权限位；含 AES-256 加密包；`-mhe=on` 加密文件头的包带密码可预览（`wispzip l -p`） |
| rar | ➖ | ✅ | 解压 rar4/rar5（含加密包，`unrar` 库）；压缩为专有格式不提供 |
| 分卷 | ✅ | ✅ | `--split 100M` 拆成 `.001/.002/…` 字节级分卷，解压时透明重组，任意内部格式 |

通用能力：

- **内容预览**：不解压即可查看条目列表（名称 / 大小 / 时间），支持勾选部分条目解压
- **进度与取消**：条目级 + 流级检查点，桌面端与 CLI 都能即时中断
- **自动多线程**：zip 解压按条目数 / 总大小 / CPU 核数 / 系统负载自动决定
  并发度（条目 ≥16 且总量 ≥1MB 才启用，系统繁忙时自动降为单线程），
  大文件优先调度；tar/7z/rar 受格式与解码器限制保持串行
- **安全**：zip-slip 防护（拒绝 `../`、绝对路径、盘符），写盘前全部走路径校验
- **重名策略**：跳过 / 覆盖 / 自动改名 `name (1).ext`
- **GBK 文件名兼容**：中文 Windows 工具打包的 zip 不会乱码
- **分卷压缩**：`--split 100M` 产出 `backup.7z.001/.002/…`，解压任一分卷自动重组；缺卷、跳号有明确报错
- **桌面端集成**：
  - 悬浮窗主界面：拖普通文件 → 自动进压缩确认页；拖压缩包 → 自动进解压确认页；确认页只问「叫什么 / 放哪里」，格式、等级、密码、分卷全部收在折叠的"高级配置"里
  - 执行页支持**暂停 / 继续 / 取消**，显示进度、速度与剩余时间；完成页给出压缩比与"打开文件夹"
  - 最近任务列表（本机存储），点击可在文件管理器中定位
  - 系统托盘（关闭窗口驻留，托盘菜单显示/退出）
  - **双击关联文件**（.zip/.7z/.rar…）直接唤起解压确认页（macOS Finder / Windows 均可）
  - **macOS 访达右键**：安装时自动注册快速操作「压缩为 ZIP」「解压」（访达右键 → 快速操作/服务；如未显示，在 系统设置 → 扩展 → Finder 中勾选启用，或应用内 设置 → 访达右键 → 重装）
  - **Windows 右键菜单**（NSIS 安装器自动注册）：文件/文件夹「压缩为 ZIP」，压缩包「解压到当前文件夹」
- CLI 快捷语义：`wispzip c <文件>` 省略 `-o` 时自动写 `<文件>.zip`；`wispzip x <包> --here` 解压到当前文件夹

## 目录结构

```
wispzip/
├── Cargo.toml               # Rust workspace（release: lto + opt-level=s + strip）
├── crates/
│   ├── wispzip-core/        # 核心引擎：编排、格式实现、进度、取消、安全
│   │   └── src/formats/     # 每个格式一个模块（zip/tar/streams/sevenz/rar），
│   │                        # 统一暴露 write/extract/list，新增格式只需加模块
│   └── wispzip-cli/         # CLI 壳（clap）
├── src-tauri/               # Tauri 2 桌面壳：命令 + 进度事件 + 任务取消
├── apps/desktop/            # Vue 3 前端（唯一一套 UI）
```

## 开发

依赖：Rust (stable)、Node 20+、pnpm。

```bash
pnpm install          # 前端依赖
pnpm tauri dev        # 桌面端开发模式（vite + cargo 同时跑）
cargo test -p wispzip-core   # 核心引擎测试（约 80 个）

# 测试分三层：
#   单元测试（src 内联）    —— 嗅探、路径安全、分卷、进度、时间换算等
#   roundtrip.rs            —— 全格式往返 / 密码 / 冲突矩阵 / zip-slip / GBK 文件名
#   interop.rs              —— 与真实工具互操作（7z/xz/bzip2/gzip/tar 双向），
#                              对应工具未安装时自动跳过，不阻塞 CI
```

## 构建

```bash
pnpm tauri build      # 产出 .app/.dmg（macOS）、NSIS/MSI（Windows）、deb/AppImage（Linux）
cargo build -p wispzip-cli --release   # 独立 CLI 二进制
```

> macOS 上 DMG 封装若因「Finder 自动化权限」弹窗未授权而失败，
> 用 `pnpm tauri build --bundles app` 只产出 .app 即可（CI 环境不受影响）。

release profile 开启 `lto + codegen-units=1 + opt-level=s + strip`，
实测 macOS aarch64 主二进制约 **5.7 MB**（GUI 二进制内嵌 CLI 逻辑，两者共用同一引擎）。

图标资源已随仓库提交；`src-tauri/icons/source.svg` 是矢量设计源。
生成脚本不入库（见 `.gitignore`）。

## CLI 用法

```bash
wispzip compress -o out.tar.gz src/ file.txt   # 格式由扩展名推断
wispzip c -o backup.zip -l 9 -p 密码 docs/     # zip AES-256 加密
wispzip list backup.zip                        # 预览内容
wispzip extract backup.zip -o 目标目录         # 解压
wispzip x legacy.zip --encoding gbk            # 老 GBK 文件名
wispzip x big.7z --entry dir/file.txt          # 只解压部分条目
wispzip c -o backup.7z --split 100M data/      # 100MB 分卷：backup.7z.001 .002 …
wispzip x backup.7z.001                        # 解压分卷（自动重组）
wispzip l backup.rar                           # rar 只读预览
wispzip x backup.rar -p secret                 # 带密码解压
```

## 许可证

MIT，详见 [LICENSE](LICENSE)。

### 第三方声明

`wispzip-core` 链接 [`unrar`](https://crates.io/crates/unrar) crate，其中捆绑 RARlab 的 UnRAR 源码。
UnRAR **不是** OSI 认可的开源许可：允许用于解压 RAR 归档，但不得用于实现 RAR 压缩器。
本仓库其余代码均为 MIT 许可。

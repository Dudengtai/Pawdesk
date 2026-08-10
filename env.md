# 桌面快速访问互动宠物开发环境与部署指南

## 1. 文档说明

| 项目 | 内容 |
| --- | --- |
| 项目名称 | 桌面快速访问互动宠物 |
| 目标平台 | Windows 10/11，x64 |
| 开发语言 | Rust |
| 核心窗口技术 | `winit` |
| 核心渲染技术 | `wgpu` |
| 文档用途 | 初始化开发环境、构建项目和部署运行环境 |
| 依据文档 | `tech.md` |

本文档只描述软件环境和安装部署要求，不描述业务功能实现。

## 2. 环境组成

### 2.1 必须安装

| 软件 | 用途 |
| --- | --- |
| Windows 10/11 x64 | 开发、构建和运行目标平台 |
| Rust 工具链 | 编译 Rust 源码、运行测试和生成发布包 |
| MSVC C++ 构建工具 | Rust `stable-x86_64-pc-windows-msvc` 工具链的链接与原生依赖编译 |
| Windows SDK | Windows API、系统库和桌面程序构建支持 |
| Git | 获取源码、管理版本和依赖变更，非运行时必需 |

### 2.2 项目依赖

项目创建后，Rust 依赖由 `Cargo.toml` 管理，主要包括：

- `winit`：窗口、事件循环、鼠标输入和窗口位置。
- `wgpu`：GPU 设备初始化、纹理、渲染管线和绘制提交。
- `windows`：Windows API、屏幕信息、进程启动和快捷方式相关能力。
- `tray-icon`：系统托盘图标和菜单。
- `serde`：配置结构序列化与反序列化。
- `serde_json` 或 TOML 库：配置文件存储格式。
- `tracing`：应用日志和诊断信息。
- `uuid`：快捷方式条目的稳定标识。
- `chrono` 或等价时间库：提醒时间和配置持久化。

如果设置页面采用 `egui`，还需要安装对应的 `egui`、平台集成和渲染集成依赖。设置页面依赖不应与宠物主窗口的 `wgpu` 渲染器产生强耦合。

## 3. 硬件要求

### 3.1 开发机最低要求

- 64 位 Windows 10 或 Windows 11。
- 双核或以上 CPU。
- 8 GB 内存，推荐 16 GB。
- 至少 5 GB 可用磁盘空间，用于 Rust 工具链、Windows SDK、依赖缓存和构建产物。
- 支持 DirectX 12、Vulkan 或 OpenGL 后端之一的图形设备。

### 3.2 运行机建议

- 64 位 Windows 10 或 Windows 11。
- 4 GB 以上内存。
- 安装正常工作的显卡驱动。
- 支持 `wgpu` 可用后端的 GPU；无法使用硬件后端时，应在应用中提供错误日志和友好提示。
- 普通用户权限即可运行，不要求管理员权限。

## 4. 安装 Windows 构建环境

### 4.1 安装 Visual Studio Build Tools

安装 Visual Studio Build Tools，并选择以下工作负载和组件：

- 使用 C++ 的桌面开发。
- MSVC x64/x86 构建工具。
- Windows 10 SDK 或 Windows 11 SDK。
- C++ CMake tools for Windows，可选但推荐。
- Git，可使用独立 Git 安装程序，也可使用 Visual Studio 自带组件。

不需要完整安装 Visual Studio IDE。若已经安装 Visual Studio Community/Professional/Enterprise，确认已启用上述 C++ 桌面开发工作负载即可。

安装完成后重新打开 PowerShell，使环境变量和工具链配置生效。

### 4.2 验证 MSVC 与 Windows SDK

在 PowerShell 中执行：

```powershell
where.exe cl
where.exe link
where.exe rc
```

如果这些命令在普通 PowerShell 中不可用，可以从开始菜单打开对应的 **Developer PowerShell for Visual Studio**，再执行验证命令。

## 5. 安装 Rust

### 5.1 推荐工具链

项目使用 MSVC 目标：

```text
stable-x86_64-pc-windows-msvc
```

安装 Rustup 后执行：

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup default stable-x86_64-pc-windows-msvc
rustup target add x86_64-pc-windows-msvc
```

### 5.2 验证 Rust

```powershell
rustc --version
cargo --version
rustup show
```

确认 `rustup show` 中的默认工具链为 `stable-x86_64-pc-windows-msvc`。

### 5.3 推荐 Rust 组件

```powershell
rustup component add rustfmt
rustup component add clippy
```

用途：

- `rustfmt`：统一格式化 Rust 源码。
- `clippy`：发现常见错误、可维护性问题和潜在性能问题。

## 6. 安装开发辅助工具

### 6.1 Git

验证 Git：

```powershell
git --version
```

建议在首次使用前配置提交身份：

```powershell
git config --global user.name "Your Name"
git config --global user.email "you@example.com"
```

### 6.2 编辑器

推荐使用以下任一编辑器：

- Visual Studio Code + `rust-analyzer` 扩展。
- RustRover。
- Visual Studio + Rust 相关扩展。

编辑器至少应支持：

- Rust 语法分析和跳转。
- `cargo check`、`cargo test` 和 `cargo run`。
- 格式化和 Clippy 诊断。
- PowerShell 终端。

### 6.3 图形调试工具

以下工具为可选项：

- RenderDoc：分析 `wgpu` 绘制调用、纹理和渲染管线。
- Windows 任务管理器：观察 CPU、内存和 GPU 使用情况。
- Process Explorer：诊断进程、句柄和资源占用。
- PIX for Windows：进行更深入的 DirectX/GPU 分析。

首期开发不要求安装图形调试工具；出现渲染问题或性能问题时再安装。

## 7. 创建项目并安装 Rust 依赖

在项目根目录执行：

```powershell
cargo new --bin pawdesk
cd pawdesk
```

如果项目已经存在，不要重复执行 `cargo new`，直接进入包含 `Cargo.toml` 的目录。

安装依赖时以 `Cargo.toml` 和 `Cargo.lock` 为准。首次构建：

```powershell
cargo check
cargo build
```

建议在项目稳定后提交 `Cargo.lock`，确保团队和发布构建使用一致的依赖版本。

## 8. `wgpu` 运行环境

### 8.1 图形后端

`wgpu` 可能使用不同的系统图形后端。Windows 环境下优先使用可用的原生后端，具体由 `wgpu` 版本和运行设备决定。开发时应至少验证：

- 支持的 GPU 驱动环境。
- 无独立显卡设备或集成显卡环境。
- 远程桌面或虚拟机环境。
- 显卡驱动异常时的错误处理。

### 8.2 驱动要求

- 安装显卡厂商提供的稳定版驱动。
- 不要将开发机固定在过旧的驱动版本。
- 发布前至少在一台集成显卡设备和一台独立显卡设备上验证。
- 如果应用初始化 `wgpu` 失败，必须记录适配器、后端和初始化错误。

### 8.3 图形初始化验证

实现最小窗口和 `wgpu` 初始化后，使用以下命令运行：

```powershell
cargo run
```

验证内容：

- 窗口可以创建并显示。
- 透明背景正常。
- 交换链或 surface 配置成功。
- 可以绘制一帧纯色或测试精灵。
- 关闭窗口不会造成进程残留。

## 9. Windows 专用能力验证

项目需要使用 Windows API 完成窗口、屏幕、快捷方式和进程启动等功能。开发阶段应验证：

- 普通用户权限可以启动应用。
- 可以获取当前显示器和工作区边界。
- 可以保存和恢复窗口位置。
- 可以启动用户主动添加的可执行文件或快捷方式。
- 目标文件不存在时可以返回可读错误。
- 托盘图标可以创建、响应菜单命令并退出应用。

快捷方式启动必须通过受控的进程 API 完成，不应将用户输入拼接成未经校验的 shell 命令。

## 10. 日志与配置目录

开发和运行时建议使用以下目录：

```text
%APPDATA%/PawDesk/config.json
%APPDATA%/PawDesk/backups/config.json.bak
%LOCALAPPDATA%/PawDesk/logs/app.log
```

配置目录由应用首次启动时自动创建。部署测试时确认：

- 目录不存在时可以自动创建。
- 配置文件损坏时可以加载备份或默认配置。
- 普通用户对目录具有读写权限。
- 卸载或删除应用程序时不会误删用户选择的外部快捷方式。

## 11. 常用开发命令

### 11.1 检查与构建

```powershell
cargo check
cargo build
cargo build --release
```

### 11.2 运行与日志

```powershell
cargo run
cargo run --release
```

### 11.3 格式化与静态检查

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

### 11.4 测试

```powershell
cargo test
cargo test --all-features
```

## 12. 发布构建环境

### 12.1 发布目标

首期发布目标：

```text
x86_64-pc-windows-msvc
```

发布构建：

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

默认可执行文件位于：

```text
target/x86_64-pc-windows-msvc/release/
```

### 12.2 发布包内容

发布包至少包含：

- 主程序 `.exe`。
- 宠物动画资源。
- 菜单图标和托盘图标。
- 必需的字体或字体资源。
- 版本信息或构建信息文件。

配置文件不应预置用户机器路径。用户配置应在首次运行时生成到应用数据目录。

### 12.3 本地三个 exe 的关系

| 路径 | 怎么生成 | 什么时候用 |
| --- | --- | --- |
| `target/debug/pawdesk.exe` | `cargo build` / `cargo run` | 开发调试 |
| `target/release/pawdesk.exe` | `cargo build --release` | 验收最新功能、日常自用 |
| `dist/PawDesk/pawdesk.exe` | `tools/package.ps1` | 便携分发；**不是** cargo 自动产物 |

注意：只编 release **不会**更新 `dist/`。要刷新便携包：

```powershell
powershell -ExecutionPolicy Bypass -File tools/package.ps1
```

日常验证最新修复请优先：

```powershell
cargo build --release
.\target\release\pawdesk.exe
```

### 12.4 便携版部署

首期优先使用便携版（或直接跑 release）：

1. 在干净目录中复制发布程序和资源目录（或运行 `package.ps1`）。
2. 不要求管理员权限运行。
3. 启动程序并验证资源加载。
4. 验证配置目录自动创建。
5. 验证快捷方式添加（**桌面用户+公共快捷方式均可见**）、启动、删除和排序。
6. 验证「添加应用」时 launcher **不闪**。
7. 验证托盘退出和再次启动。

## 13. 安装包部署

当前**未实现**正式安装包；需要时可选用 Inno Setup / WiX 等。若后续制作安装包，安装程序应满足：

- 支持 Windows 10/11 x64。
- 默认安装到用户可写目录或正确处理安装权限。
- 不强制要求管理员权限，除非明确提供系统级安装功能。
- 可以创建开始菜单快捷方式。
- 可选提供开机启动。
- 卸载时保留或明确询问是否删除用户配置。
- 清理临时文件和安装器产生的无用资源。

安装包工具不在当前技术方案中强制指定，可根据发布方式选择 WiX、Inno Setup 或其他 Windows 安装方案。

## 14. 环境验证清单

### 14.1 开发机检查

- [ ] Windows 为 64 位 Windows 10/11。
- [ ] Visual Studio C++ 桌面开发工作负载已安装。
- [ ] Windows SDK 已安装。
- [ ] `rustc --version` 可以执行。
- [ ] `cargo --version` 可以执行。
- [ ] 默认 Rust 工具链为 MSVC。
- [ ] `cargo check` 成功。
- [ ] `cargo test` 成功。
- [ ] `cargo clippy` 无阻断错误。
- [ ] `cargo build --release` 成功。

### 14.2 运行机检查

- [ ] 可以启动发布版 `.exe`。
- [ ] 宠物透明窗口正常显示。
- [ ] 动画资源加载正常。
- [ ] 鼠标拖动和点击正常。
- [ ] 系统托盘正常显示。
- [ ] 配置目录可以创建和写入。
- [ ] Windows 快捷方式可以启动。
- [ ] 多显示器位置约束正常。
- [ ] 退出后没有残留进程。

### 14.3 性能检查

- [ ] 待机 CPU 占用满足目标。
- [ ] 常驻内存满足目标。
- [ ] 菜单展开和关闭无明显卡顿。
- [ ] 连续运行 10 分钟后动画和提醒状态正常。
- [ ] 系统休眠/唤醒后计时器状态正常。

## 15. 常见问题

### 15.1 `link.exe` 找不到

原因通常是未安装 MSVC C++ 构建工具，或当前终端没有加载 Visual Studio 开发环境。

处理方式：

1. 安装“使用 C++ 的桌面开发”。
2. 安装对应 Windows SDK。
3. 重新打开 Developer PowerShell。
4. 再执行 `cargo build`。

### 15.2 `wgpu` 初始化失败

检查显卡驱动、运行环境和日志。优先在本机直接运行，而不是远程桌面或虚拟机中运行。确认应用记录了适配器和图形后端初始化错误。

### 15.3 透明窗口显示黑色背景

检查窗口透明属性、surface 配置、alpha 合成方式和绘制目标格式。透明窗口相关能力应集中封装在窗口和渲染模块中，避免业务层分散处理。

### 15.4 快捷方式无法启动

确认目标路径存在、参数解析正确、工作目录有效，并检查当前用户是否具有执行权限。不要直接把整条快捷方式内容拼接到 shell 命令中。

## 16. 环境版本管理建议

- 使用 `rust-toolchain.toml` 固定 Rust 工具链。
- 将 `Cargo.lock` 纳入版本控制。
- 在 CI 和本地使用相同的 target triple。
- 发布构建使用固定的 release 配置。
- 升级 `winit`、`wgpu` 或 Windows API 依赖时，单独验证透明窗口、托盘、GPU 初始化和多显示器行为。
- 升级依赖前保留可回滚的锁文件和发布构建记录。

建议的 `rust-toolchain.toml` 示例：

```toml
[toolchain]
channel = "stable"
targets = ["x86_64-pc-windows-msvc"]
components = ["rustfmt", "clippy"]
```

## 17. 最小初始化流程

在新开发机上按以下顺序执行：

```powershell
# 1. 验证 Rust 和 MSVC
rustc --version
cargo --version
where.exe cl

# 2. 进入项目目录
cd D:\AI练习目录\PawDesk

# 3. 检查依赖和源码
cargo check

# 4. 执行测试
cargo test

# 5. 构建调试版本
cargo build

# 6. 构建发布版本
cargo build --release --target x86_64-pc-windows-msvc
```

如果项目尚未创建 `Cargo.toml`，应先按照技术设计文档完成 Rust 项目骨架，再执行上述命令。

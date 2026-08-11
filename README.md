# PawDesk

Windows 桌面轻量互动宠物：陪伴待机 + 快捷启动坞 + 健康提醒。

## 快速开始

```powershell
# 需要：Rust stable-msvc、Windows SDK
cargo run --release
```

环境与部署细节见 [`docs/env.md`](docs/env.md)。

## 文档

项目文档统一放在 **[`docs/`](docs/)**，避免根目录堆满 `.md`：

| 文档 | 内容 |
| --- | --- |
| [docs/prd.md](docs/prd.md) | 产品需求 |
| [docs/tech.md](docs/tech.md) | 技术设计 |
| [docs/design.md](docs/design.md) | 视觉与交互 |
| [docs/task.md](docs/task.md) | 开发计划 / 下一步 |
| [docs/env.md](docs/env.md) | 环境与部署 |
| [docs/mockups/](docs/mockups/) | 启动坞效果图 |

完整索引：[`docs/README.md`](docs/README.md)。

## 仓库结构（摘要）

```text
PawDesk/
├── README.md          ← 你在这里
├── Cargo.toml
├── assets/            ← 宠物精灵、字体、托盘图标
├── docs/              ← 全部项目文档
├── examples/
├── src/               ← Rust 源码
└── tools/             ← 资源管线脚本
```

## 许可与状态

个人 / 练习项目。里程碑与当前进度见 [`docs/task.md`](docs/task.md)。

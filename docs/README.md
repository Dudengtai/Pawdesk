# PawDesk 项目文档

所有产品 / 技术 / 设计 / 排期文档集中在本目录，仓库根目录只保留工程入口（`Cargo.toml`、`src/`、`assets/` 等）。

## 文档索引

| 文档 | 说明 | 谁在什么时候读 |
| --- | --- | --- |
| [prd.md](./prd.md) | **产品需求（PRD）** — 做什么、为谁、做到什么程度 | 改需求先改这里；产品真相源 |
| [tech.md](./tech.md) | **技术设计** — 架构、模块、窗口、状态机、性能 | 写代码 / 排风险前 |
| [design.md](./design.md) | **视觉与交互** — 色板、动效、启动坞、提醒 UI | 改 UI / 对照 mockup |
| [task.md](./task.md) | **开发计划** — 里程碑、任务勾选、下一步 | 排期与验收进度 |
| [env.md](./env.md) | **环境与部署** — 工具链、构建、发布 | 新机器开工 / 打发布包 |

## 效果图

| 路径 | 说明 |
| --- | --- |
| [mockups/](./mockups/) | 启动坞 HTML/PNG 原型（`launcher-preview` · `launcher-pin-flip`） |

## 阅读顺序（新人）

1. `prd.md` — 产品边界  
2. `design.md` §5 + `mockups/` — 启动坞长什么样  
3. `tech.md` §1–§5 — 怎么实现  
4. `task.md` §1 + 当前「下一步」— 现在该干什么  
5. `env.md` — 本地怎么跑起来  

## 变更约定

- 改行为：先 `prd.md` → 再 `tech.md` / `design.md` → 再 `task.md` 勾状态 → 最后改代码。  
- 仅视觉：可只改 `design.md` + mockup。  
- 仅实现重构且行为不变：可只改 `tech.md`。  
- 文档互链使用**同目录相对文件名**（如 `` `tech.md` ``）；mockup 使用 `` `mockups/...` ``。

## 目录结构

```text
docs/
├── README.md          ← 本索引
├── prd.md
├── tech.md
├── design.md
├── task.md
├── env.md
└── mockups/
    ├── launcher-preview.html / .png
    └── launcher-pin-flip.html / .png
```

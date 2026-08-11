# PawDesk 开发计划（按模块）

## 1. 文档信息

| 项目 | 内容 |
| --- | --- |
| 项目名称 | 桌面快速访问互动宠物（PawDesk） |
| 文档类型 | 开发任务与模块排期 |
| 当前版本 | **v0.9**（2026-08-11：精灵去品红边 / 软边 / 伸懒腰无幻影 / 固定朝向 · 文档归入 `docs/`） |
| 依据文档 | `prd.md` v0.5、`tech.md` v0.7、`design.md` v0.9 |
| 环境参考 | `env.md` |
| 状态 | **M0–M5 可日常使用**；**M6 坞精致化主路径已落地**；**cow-cat 精灵桌面可读性收口**；字体更精致化后期 |
| **下一步** | 观察 `idle_blink` + `idle_stretch` 日常观感；通过后再开 cute/wag/sleep；可选：飞扑重开 / 更多 oneshot |

### 1.1 使用说明

- 任务按 **模块** 组织，并映射到 **里程碑 M0–M5**；实现时按里程碑依赖顺序推进，模块内可并行的已标注。
- 状态约定：`[ ]` 未开始 · `[~]` 进行中 · `[x]` 完成 · `[-]` 取消/延期
- 优先级：`P0` 首期必须 · `P1` 首期应有 · `P2` 可后置
- 完成定义：对应「完成标准」全部满足，且相关单元/手工验收通过。
- 变更产品行为时：先改 `prd.md`，再同步本文件与 `tech.md` / `design.md`。

### 1.2 版本记录

| 版本 | 日期 | 说明 |
| --- | --- | --- |
| v0.1 | 2026-08-01 | 根据 prd/tech/design 首期范围生成模块化开发计划 |
| v0.2 | 2026-08-04 | 记录 M0–M4 完成态 + 宠物/动画管线 + 快捷启动 Apple 风 UI 进度 |
| v0.3 | 2026-08-04 | M5 交付同步；明确下阶段：修 bug / 交互 / UI，宠物形象动作最后 |
| v0.4 | 2026-08-04 | 拍板启动坞 **钉宠 + Flip/Shift**；新增 §14 开发计划；对齐 design/tech §7 |
| v0.5 | 2026-08-06 | 待机真眨眼资源 + clip 播放 polish；`pet.scale` 默认 0.6 + 设置/托盘手动调节；文档同步 prd/design/tech |
| v0.5.1 | 2026-08-06 | 记录 **下一步 = 随机撒娇动画效果优化**（PET-A07）；release 便携包已打 |
| v0.6 | 2026-08-07 | 启动坞 Appica 精致 UI + 丝滑开合；MENU-09~13 / L6；design v0.7 · tech v0.5 |
| v0.7 | 2026-08-07 | **坞 bug 收口**：flat 主按钮；GDI 文字（fontdue 弃用于 UI）；列表滚轮可扩展（非封顶 5）；design **v0.8** · tech **v0.6**；L7 |
| **v0.8** | **2026-08-10** | 添加应用：原生 `IFileOpenDialog`、Shell 虚拟桌面、取消 z-order 闪烁；design **v0.9** · tech **v0.7** |
| **v0.9** | **2026-08-11** | **精灵 polish**：品红描边 despill、全帧软边 AA、双线性缩放；`idle_stretch` 倒放回坐（去掉 AI 多头幻影）；取消鼠标驱动左右翻转（固定素材朝向）；工具脚本 `despill_pet_edges` / `soften_pet_edges` / `fix_stretch_return`；**文档迁入 `docs/`** 并补根 `README.md` 索引 |

### 1.3 2026-08-11 精灵与呈现收口（已落地）

| 项 | 状态 | 说明 |
| --- | --- | --- |
| 品红 / 粉紫轮廓 | [x] | `tools/despill_pet_edges.py` 全 `cow-cat` 帧 |
| 硬边锯齿 | [x] | 素材 Gaussian 软边 + `scale_rgba_centered` 预乘双线性 |
| 伸懒腰结束幻影 | [x] | 回坐改为干净前半段倒放；`go_idle` 硬切 base，无 clip crossfade |
| 鼠标左右朝向反了 | [x] | 禁用 `mirror_x` 与 cursor `face_dir` 翻转，固定素材朝向 |
| 文档杂乱 | [x] | `prd/tech/design/task/env` → `docs/`；`docs/README.md` + 根 `README.md` |

---

## 2. 里程碑总览

对齐 PRD §8 与 tech §18：

| 里程碑 | 目标 | 主要模块 | 完成标志（产品） |
| --- | --- | --- | --- |
| **M0** | 工程骨架与技术验证 | foundation, platform, render 最小集, tray 最小集 | 透明窗 + 单帧猫 + 拖动 + 日志 + 托盘退出 |
| **M1** | 桌面宠物可见可拖 | pet_window, pet/animation, config 位置 | 多套待机动画 + 拖动 + 位置持久化 |
| **M2** | 互动成型 | pet/state, movement, interaction | 边缘探头 + 鼠标扑近互动 |
| **M3** | 提醒闭环 | reminder, pet 提醒态, design 提醒 UI | 定时提醒 + 投喂 + 回位 + 暂停 |
| **M4** | 快捷访问 | shortcut, radial_menu, settings 快捷管理 | 菜单 + 增删排序启动 + 持久化 |
| **M5** | 可日常使用 | tray 完整, settings 完整, 性能/测试/发布 | 托盘全能力 + 设置 + 性能达标 + 发布包 |

```text
M0 ──► M1 ──► M2 ──► M3 ──► M4 ──► M5
              │              │
              │              └─ shortcut 数据层可与 M3 后期并行
              └─ assets 可与各阶段并行补齐
```

---

## 3. 模块依赖关系

```text
foundation (工程/错误/日志/事件总线)
    │
    ├─► platform/windows
    │       │
    │       └─► ui/pet_window ──► render (wgpu 绘制抽象)
    │               │
    ├─► config ─────┼─► pet/* (state, animation, movement, interaction)
    │               │
    ├─► reminder ───┘
    │
    ├─► shortcut ──► ui/radial_menu
    │                    │
    ├─► ui/tray ─────────┤
    │                    │
    └─► ui/settings ◄────┘
              │
              └─► assets (美术资源贯穿全期)
```

| 模块 ID | 对应源码（tech） | 职责摘要 |
| --- | --- | --- |
| `MOD-FOUND` | `main.rs`, `app.rs`, `error.rs` | 入口、生命周期、事件、统一错误 |
| `MOD-PLAT` | `platform/windows.rs` | 透明/置顶/工作区/DPI/多屏/进程相关 |
| `MOD-CFG` | `config/*` | 配置模型、读写、迁移、防抖保存 |
| `MOD-RENDER` | Renderer trait + wgpu 实现 | 精灵/文字/面板/按钮绘制 |
| `MOD-PET` | `pet/*` | 状态机、动画、移动、边缘、鼠标互动 |
| `MOD-WIN` | `ui/pet_window.rs` | 宠物主窗口、命中、拖动、输入分发 |
| `MOD-RM` | `reminder/*` | 调度、文案、投喂闭环事件 |
| `MOD-SC` | `shortcut/*` | 快捷方式模型、持久化、启动 |
| `MOD-MENU` | `ui/radial_menu.rs` | 径向菜单与自适应展开 |
| `MOD-TRAY` | `ui/tray.rs` | 系统托盘 |
| `MOD-SET` | `ui/settings.rs` | 设置与快捷方式管理 UI |
| `MOD-ASSET` | `assets/*` | 奶牛猫动画、菜单/托盘图标、元数据 |
| `MOD-QA` | tests + 手工验收 | 测试、性能、发布 |

---

## 4. 按模块任务清单

### 4.1 MOD-FOUND - 工程基础

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| FOUND-01 | 初始化 Cargo 工程（Windows x64、edition/工具链按 env） | M0 | P0 | - | env, tech §4 | [x] |
| FOUND-02 | 建立目录骨架：`app` / `config` / `pet` / `reminder` / `shortcut` / `ui` / `platform` | M0 | P0 | FOUND-01 | tech §5 | [x] |
| FOUND-03 | 统一错误类型 `AppError` 与用户可读错误映射 | M0 | P0 | FOUND-01 | tech §13 | [x] |
| FOUND-04 | 接入 `tracing` 日志（级别、文件路径 `%LOCALAPPDATA%/PawDesk/logs`） | M0 | P0 | FOUND-01 | tech §11, §13 | [x] |
| FOUND-05 | 定义 `AppEvent` / `TrayCommand` 等消息枚举与主循环分发骨架 | M0 | P0 | FOUND-02 | tech §12 | [x] |
| FOUND-06 | `app.rs` 生命周期：启动初始化 -> 事件循环 -> 优雅退出 | M0 | P0 | FOUND-05 | tech §4.2 | [x] |
| FOUND-07 | 后台任务通道约定：I/O 不进主线程渲染路径 | M0 | P1 | FOUND-05 | tech §4.2 | [x] |

**完成标准**

- [x] `cargo build` 通过；能跑空事件循环并写日志
- [x] 错误与日志路径符合 tech，无需管理员权限

---

### 4.2 MOD-PLAT - Windows 平台能力

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| PLAT-01 | 封装工作区/虚拟桌面坐标、主显示器与多显示器枚举 | M0 | P0 | FOUND-02 | tech §7.1 | [x] |
| PLAT-02 | 窗口属性：无边框、透明、置顶、不强制抢焦点 | M0 | P0 | PLAT-01 | tech §7.1, prd F-UI-01 | [x] |
| PLAT-03 | DPI 缩放系数读取；逻辑尺寸 -> 物理像素换算 | M1 | P0 | PLAT-02 | design §11, prd §6.5 | [x] |
| PLAT-04 | 鼠标穿透策略：透明区尽量不拦截，实体区可点 | M1 | P0 | PLAT-02 | tech §7.1, design §5.1 | [x] |
| PLAT-05 | 显示器变更时将位置钳制回可用工作区 | M5 | P0 | PLAT-01 | tech §7.1, prd Q5 | [x] |
| PLAT-06 | 文件选择器（添加快捷方式） | M4 | P0 | FOUND-02 | tech §10.1 | [x] |
| PLAT-07 | 进程启动封装（非 shell 拼接）与失败原因分类 | M4 | P0 | FOUND-03 | tech §10.3, §14 | [x] |

**完成标准**

- [ ] 透明悬浮窗无黑边（目标机手工确认）
- [ ] 透明区域不误挡桌面操作
- [ ] 多屏拔插后宠物仍可找回（M5）

---

### 4.3 MOD-CFG - 配置与持久化

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| CFG-01 | 定义 `AppConfig` / `PetConfig` / `ReminderConfig` / `WindowConfig` + `schema_version` | M1 | P0 | FOUND-02 | tech §6.3 | [x] |
| CFG-02 | 默认配置与路径：`%APPDATA%/PawDesk/config.json` | M1 | P0 | CFG-01 | tech §11.1 | [x] |
| CFG-03 | 原子写入 + `.bak` 备份；读失败回退备份/默认 | M1 | P0 | CFG-02 | tech §11.2 | [x] |
| CFG-04 | 防抖保存（拖动位置、排序等） | M1 | P0 | CFG-03 | tech §4.2 | [x] |
| CFG-05 | 窗口位置/显示器信息读写与恢复 | M1 | P0 | CFG-04, PLAT-01 | prd F-UI-03, S1 | [x] |
| CFG-06 | `migration.rs`：旧 schema 升级路径 | M5 | P1 | CFG-01 | tech §6.3 | [x] |
| CFG-07 | 提醒配置字段：`enabled` / `interval_minutes` / `last_completed_at` / 文案列表 | M3 | P0 | CFG-01 | tech §6.3, prd Q1 | [x] |
| CFG-08 | 快捷方式列表嵌入配置或独立存储策略落地 | M4 | P0 | CFG-01 | tech §6.2 | [x] |

**完成标准**

- [x] 杀进程再开，位置与关键配置仍在
- [x] 损坏配置可回退且不崩溃

---

### 4.4 MOD-RENDER - 渲染层

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| RND-01 | `wgpu` 初始化与交换链（适配透明窗） | M0 | P0 | PLAT-02 | tech §4.1 | [x] |
| RND-02 | 实现 `Renderer`：`draw_sprite` / `draw_text` / `draw_panel` / `draw_button` | M0–M1 | P0 | RND-01 | tech §4.1, design §2 | [x] |
| RND-03 | 纹理加载、缓存、精灵图 UV 采样 | M1 | P0 | RND-02, ASSET-01 | tech §8.1–8.2 | [x] |
| RND-04 | 缓动常量：`ease.snappy` / `ease.smooth` | M1 | P1 | RND-02 | design §3.5 | [x] |
| RND-05 | 动态刷新率策略：待机 12–24 FPS，移动/菜单提高 | M1 | P1 | RND-02 | tech §8.3 | [x] |
| RND-06 | 设计 token 色板与浅/深主题（面板用） | M3–M4 | P1 | RND-02 | design §3.2 | [ ] |
| RND-07 | 无变化时跳过强制重绘 | M5 | P1 | RND-05 | tech §8.3 | [x] |

**完成标准**

- [ ] 业务层不直接碰 GPU 细节，只走 `Renderer` 接口
- [ ] 待机动画流畅且占用可控（最终以 M5 指标验收）

---

### 4.5 MOD-ASSET - 资源与美术对接

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| ASSET-01 | 资源目录与动画 JSON 元数据规范（帧尺寸、fps、loop、anchor） | M0 | P0 | FOUND-02 | tech §8.1, design §13 | [x] |
| ASSET-02 | 奶牛猫占位单帧 / 占位精灵（可先程序员美术） | M0 | P0 | ASSET-01 | tech 阶段一 | [x] |
| ASSET-03 | 待机：`idle_blink`（真眨眼）+ 30s 池 + watch；256 帧图 | M1 | P0 | ASSET-01 | prd F-AN-01, tech §3.2 | [x] 2026-08-06 真眨眼重建 |
| ASSET-04 | 互动 / 扑近 / 提醒 / 投喂相关帧 | M2–M3 | P0 | ASSET-03 | design §4.2, §6 | [x] |
| ASSET-05 | 拖动、边缘探头状态视觉帧 | M2 | P1 | ASSET-03 | design §4.2, §5.2 | [x] |
| ASSET-06 | 托盘图标 16/32 + 菜单固定入口图标 | M4 | P0 | ASSET-01 | design §9, §13 | [ ] |
| ASSET-07 | 食物按钮与提醒气泡素材 | M3 | P0 | ASSET-01 | design §6 | [x] |
| ASSET-08 | 打包路径与 release 资源完整性检查 | M5 | P0 | 全部资源 | tech §17 | [x] |

**完成标准**

- [ ] 每套动画有可加载元数据；缺失资源有日志与降级（占位帧）
- [ ] 基础尺寸对齐 design：128×128 基准（96 DPI）

---

### 4.6 MOD-PET - 宠物领域（状态机 / 动画 / 移动 / 互动）

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| PET-01 | `PetState` 枚举与合法转换表（集中状态机，禁止 UI 直改） | M1 | P0 | FOUND-05 | tech §6.1, prd §5.7 | [x] |
| PET-02 | 动画控制器：基于时间的帧推进、循环、结束回调 | M1 | P0 | RND-03, ASSET-03 | tech §8.2 | [x] |
| PET-03 | 待机 base 眨眼 + 每 **60s** one-shot（**当前仅 stretch**；池可扩） | M1 | P0 | PET-02 | tech §6.1/§8.2, design §4.3 | [x] |
| PET-04 | `Dragging`：按下移动进入、释放回 Idle；拖动反馈 | M1 | P0 | PET-01, WIN-03 | prd F-UI-02, design §10 | [x] |
| PET-05 | `HiddenAtEdge`：阈值、隐藏比例、探头命中、点击恢复 | M2 | P0 | PET-01, PLAT-01 | tech §7.2, design §5.2, prd F-AN-03 | [x] |
| PET-06 | 鼠标距离：`Watching` 已接；**飞扑 `Approaching` 延后**（`ENABLE_MOUSE_POUNCE=false`） | M2 / 后期 | P0 | PET-01 | tech §6.1, prd F-AN-04 | [~] |
| PET-07 | 移动插值与路径（扑近、回位、去中心） | M2–M3 | P0 | PET-01, RND-04 | tech movement, design ease | [x] |
| PET-08 | 状态优先级：拖动 > 提醒待办延迟；提醒 > 自动扑近；菜单打开抑制扑近 | M2–M3 | P0 | PET-01, RM-*, MENU-* | prd §5.7 | [x] |
| PET-09 | `Reminder(*)` 子阶段接入（移动/展示/返回） | M3 | P0 | PET-07, RM-02 | tech §9.2 | [x] |
| PET-10 | `MenuOpen` 状态与进出场 | M4 | P0 | PET-01 | tech §6.1 | [x] |
| PET-11 | 状态机单元测试（合法/非法转换、优先级） | M2 | P0 | PET-01 | tech §16.1 | [x] |
| PET-12 | 距离/边缘/动画冷却单元测试 | M2 | P0 | PET-05, PET-06 | tech §16.1 | [x] |

**完成标准**

- [x] 覆盖 prd 验收 2–4：待机动画、边缘探头、鼠标互动
- [x] 状态切换均有日志（debug）；无隐式双状态源

---

### 4.7 MOD-WIN - 宠物主窗口

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| WIN-01 | `winit` 宠物窗口创建并挂接 platform 属性 | M0 | P0 | PLAT-02, RND-01 | tech §7.1 | [x] |
| WIN-02 | 输入：移动/按下/释放 -> `AppEvent` | M0 | P0 | FOUND-05, WIN-01 | tech §12 | [x] |
| WIN-03 | 拖动改窗口位置 + 防抖写入 config | M1 | P0 | WIN-02, CFG-05 | prd F-UI-02/03 | [x] |
| WIN-04 | 非矩形/透明命中测试（仅宠物实体可点） | M1 | P0 | PLAT-04 | design §5.1, prd F-UI-04 | [x] |
| WIN-05 | 关闭窗口 ≠ 退出进程（隐藏或忽略，退出走托盘） | M0 | P0 | TRAY-01 | tech §7.1, prd §6.2 | [x] |
| WIN-06 | 启动 2s 内显示窗口（资源慢时用占位帧） | M1 | P1 | ASSET-02 | tech §15.1 | [x] |
| WIN-07 | 显示/隐藏宠物（托盘命令） | M5 | P0 | TRAY-02 | prd F-TR-02, S8 | [x] |

**完成标准**

- [ ] S1：启动即见猫，可拖，透明不挡桌面
- [ ] 关窗不杀进程；托盘可退

---

### 4.8 MOD-RM - 健康提醒

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| RM-01 | `scheduler`：默认 60 分钟；单调时钟；暂停/恢复整周期 | M3 | P0 | CFG-07, FOUND-05 | tech §9.1, prd F-RM-01/04 | [x] |
| RM-02 | 发布 `ReminderDue`；不直接操窗 | M3 | P0 | RM-01 | tech §9.1 | [x] |
| RM-03 | 启动补发策略：最多补一次，不连弹 | M3 | P0 | RM-01 | tech §9.1 | [x] |
| RM-04 | 提醒 UI 流程：存原位 -> 去中心 -> 动画 -> 文案 -> 食物按钮 | M3 | P0 | PET-09, RND-06, ASSET-07 | prd F-RM-02, design §6 | [x] |
| RM-05 | 内置幽默文案池（prd 四条 + 可扩展） | M3 | P0 | RM-04 | prd F-RM-03 | [x] |
| RM-06 | 投喂完成：反馈动画 -> 回位 -> 写 `last_completed_at` | M3 | P0 | RM-04, CFG-07 | tech §9.2 | [x] |
| RM-07 | 提醒中拖动不打断；结束后处理待办提醒 | M3 | P0 | PET-08 | prd F-RM-05 | [x] |
| RM-08 | 开发用「缩短间隔」调试开关（不进正式默认） | M3 | P1 | RM-01 | prd 验收 §9 | [x] |
| RM-09 | 调度/暂停/补发单元测试 | M3 | P0 | RM-01, RM-03 | tech §16.1 | [x] |
| RM-10 | （P1）稍后提醒 / 托盘重新进入提醒 | M5 | P1 | RM-02, TRAY-02 | tech §9.2, prd Q4 | [ ] |

**完成标准**

- [x] 完整投喂闭环；暂停后不再弹出
- [x] 重启后调度状态合理（不无脑连弹）

---

### 4.9 MOD-SC - 快捷方式领域

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| SC-01 | `ShortcutItem` 模型（uuid、路径、参数、排序、enabled） | M4 | P0 | CFG-08 | tech §6.2 | [x] |
| SC-02 | repository：增删改排序 + 防抖持久化 | M4 | P0 | SC-01, CFG-04 | prd F-SC-04/05 | [x] |
| SC-03 | 解析 `.lnk` / `.exe`（失败可手动修正） | M4 | P0 | PLAT-06 | tech §10.1, 风险表 | [x] |
| SC-04 | `launcher`：启动前校验；可读错误；禁危险 shell 拼接 | M4 | P0 | PLAT-07 | tech §10.3, prd 安全 | [x] |
| SC-05 | 失效路径：保留条目 + 提示修复/删除 | M4 | P1 | SC-04 | prd F-SC-07, design §7.3 | [x] |
| SC-06 | 删除仅移除应用条目，不删用户磁盘文件 | M4 | P0 | SC-02 | prd F-SC-06 | [x] |
| SC-07 | 排序与失效处理单元测试 | M4 | P0 | SC-02 | tech §16.1 | [x] |
| SC-08 | 添加应用：原生对话框 + 虚拟桌面 + 不切置顶 | M6-A | P0 | SC-03 | tech §7.4, design §5.6 | [x] |

**完成标准**

- [ ] 增删排序启动全流程可用；重启顺序不变
- [ ] 失效项可见且可处理

---

### 4.10 MOD-MENU - 径向快捷菜单

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| MENU-01 | 单击宠物 -> `MenuOpen`；空白/再点关闭 | M4 | P0 | PET-10, WIN-02 | prd F-SC-01 | [x] |
| MENU-02 | 径向布局：圆心=宠物、项尺寸/半径按 design | M4 | P0 | RND-02 | design §7.1 | [x] |
| MENU-03 | 展开方向自适应（左/右/上/下/角） | M4 | P0 | PLAT-01 | tech §7.3, design §7.2, prd F-SC-03 | [x] |
| MENU-09 | **钉宠 place_launcher**（Flip/Shift/Union） | M6-B | P0 | MENU-03 | design §5.3, tech §5.2, task §14 | [x] |
| MENU-10 | 动态 union 窗 + compose 分画宠/卡 | M6-B | P0 | MENU-09 | task §14 L1 · tech §5.3 | [x] |
| MENU-11 | Opening/Closing 丝滑动效（最终 placement 锁定；宠不闪；~60fps） | M6-B | P0 | MENU-10 | design §5.6 · tech §5.3–5.4 | [x] |
| MENU-12 | Appica 暖玻璃 + primary/soft/row 精致控件（非 Acrylic） | M6-C | P0 | MENU-10 | design §2 · §5.2–5.6 | [x] |
| MENU-13 | hover/press 插值 + 子项 stagger + 原子 present 防空帧 | M6-B | P1 | MENU-11 | design §5.6 · tech §5.4 | [x] |
| MENU-04 | 固定入口 + 动态快捷项（按 sort_order） | M4 | P0 | SC-02 | design §5.4, prd F-SC-08 | [x] |
| MENU-05 | 展开/收起动效与 100ms 内反馈（已升级丝滑 60fps） | M4 | P0 | RND-04 | design §2.5, §5.6 | [x] |
| MENU-06 | 点击项 -> `ShortcutSelected` -> 启动 | M4 | P0 | SC-04 | prd F-SC-05 | [x] |
| MENU-07 | 失效项警告态（不直接隐藏） | M4 | P1 | SC-05 | design §7.3, §12 | [x] |
| MENU-08 | 项过多时分页或滚动降级 | M4 | P1 | MENU-02 | tech §7.3 | [ ] |

**完成标准**

- [ ] 屏幕四边四角菜单不被裁切（手工）
- [ ] 风格与宠物统一，非系统原生右键菜单感

---

### 4.11 MOD-TRAY - 系统托盘

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| TRAY-01 | 托盘图标创建（可先用占位图） | M0 | P0 | FOUND-06 | tech 阶段一, prd F-TR-01 | [x] |
| TRAY-02 | 菜单：显示/隐藏、变大/变小、暂停/恢复提醒、打开设置、退出 | M0 骨架 / M5 接齐 | P0 | TRAY-01 | prd F-TR-02, design §7.2 | [x] |
| TRAY-03 | 退出：释放资源、刷配置、结束进程 | M0 | P0 | TRAY-02, CFG-04 | tech §7.1 | [x] |
| TRAY-04 | 暂停/恢复提醒与 scheduler 同步 | M3 | P0 | RM-01, TRAY-02 | prd F-RM-04 | [x] |
| TRAY-05 | 打开设置窗口 | M5 | P0 | SET-01 | prd F-ST-01 | [x] |
| TRAY-07 | 宠物变大 / 变小（写 `pet.scale` + 待机即时改窗） | M6 | P0 | SET-04 | prd F-TR-02, F-UI-05 | [x] 2026-08-06 |
| TRAY-06 | 替换为 design 规定的宠物头像托盘图 | M5 | P1 | ASSET-06 | design §9 | [x] |

**完成标准**

- [ ] S8：隐藏宠物、暂停提醒、退出均可用
- [ ] 关闭设置窗不退出应用

---

### 4.12 MOD-SET - 设置界面

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| SET-01 | 独立设置窗口（建议 egui，与主窗渲染解耦） | M4 | P0 | FOUND-06 | tech §4.1, design §8 | [x] |
| SET-02 | 快捷方式管理：列表、添加、删除确认、排序、启用开关 | M4 | P0 | SC-02, PLAT-06 | design §8.1, prd F-ST-01 | [x] |
| SET-03 | 提醒设置：开关、间隔（默认 60，范围 15–180）、文案列表（若做） | M5 | P0 | CFG-07, RM-01 | design §8.1, prd Q1 | [x] |
| SET-04 | 宠物设置：大小（步进 UI）/透明度/边缘隐藏开关/主题跟随（按 design 首期能力裁剪） | M5 | P1 | CFG-01 | design §7.1, prd F-ST-03 | [x] 大小已交付 2026-08-06；透明度/主题仍可后置 |
| SET-05 | 视觉换肤贴近 design token（非裸系统灰窗） | M5 | P1 | RND-06 | design §8.2 | [~] |
| SET-06 | 关闭设置 ≠ 退出；配置变更防抖保存 | M5 | P0 | CFG-04 | prd §6.2 | [x] |
| SET-07 | （P2）开机自启开关 | 后期 | P2 | SET-01 | prd Q2 | [ ] |

**完成标准**

- [ ] 设置内可完成快捷方式管理与提醒开关/间隔
- [ ] 改完重启仍生效

---

### 4.13 MOD-QA - 测试、性能与发布

| 任务 ID | 任务 | 里程碑 | 优先级 | 依赖 | 依据 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| QA-01 | 单元测试：状态机、调度、配置、排序（随模块补齐） | M2–M5 | P0 | 各模块 | tech §16.1, prd 验收 | [~] |
| QA-02 | 集成：配置原子写、托盘命令、快捷启动 | M5 | P0 | CFG, TRAY, SC | tech §16.2 | [ ] |
| QA-03 | 手工清单：拖动不出屏、透明命中、菜单四角、提醒冲突态 | M5 | P0 | 全功能 | tech §16.3 | [ ] |
| QA-04 | 性能基准：待机 10min、动画 10min、菜单、提醒、双屏拖动 | M5 | P0 | 全功能 | tech §15.2 | [ ] |
| QA-05 | 达标：待机 CPU < 3%、内存 < 200MB；交互 < 100ms | M5 | P0 | QA-04 | prd §6.1, tech §15.1 | [ ] |
| QA-06 | release 构建、资源打包、便携版目录结构 | M5 | P0 | ASSET-08 | tech §17 | [x] |
| QA-07 | 无管理员权限运行验证；Defender 基本启动检查 | M5 | P1 | QA-06 | tech §17.2 | [ ] |
| QA-08 | 对标 prd §9 / tech §20 验收表签字 | M5 | P0 | QA-03–06 | prd §9 | [ ] |

**完成标准**

- [ ] 首期验收 9 条全部通过（prd §9）
- [ ] 有一份简单的性能记录（机器环境 + 数值）

---

## 5. 里程碑检查清单（汇总）

### M0 - 技术验证

- [x] FOUND-01～06
- [x] PLAT-01～02
- [x] RND-01～02（最小）
- [x] WIN-01～02, WIN-05
- [x] TRAY-01～03
- [x] ASSET-01～02  
**Demo**：透明置顶猫（单帧）可拖，托盘可退出，有日志 - **已完成（2026-08-01）**

### M1 - 可见可拖 + 待机动画

- [x] CFG-01～05
- [x] PET-01～04
- [x] ASSET-03
- [x] RND-03～05
- [x] WIN-03～04, WIN-06  
**Demo**：≥5 待机动画，位置重启恢复 - **已完成（2026-08-01）**

### M2 - 互动成型

- [x] PET-05~08, PET-11~12
- [x] ASSET-04（互动部分）, ASSET-05  
**Demo**：边缘探头 + 鼠标扑近 - **已完成（2026-08-01）**

### M3 - 提醒闭环

- [x] CFG-07, RM-01～09
- [x] PET-09
- [x] ASSET-07, ASSET-04（提醒/投喂）
- [x] TRAY-04  
**Demo**：短间隔可跑通投喂闭环；可暂停 - **已完成（2026-08-01）**

### M4 - 快捷访问

- [x] SC-01～07
- [x] MENU-01～06（07 已做警告态；08 分页延后）
- [x] SET-01～02（功能向自绘管理面板，非 egui 换肤）
- [x] PLAT-06～07
- [x] CFG-08  
**Demo**：单击菜单启动软件；管理列表持久化 - **已完成（2026-08-01）**

### M5 - 可日常使用

- [x] TRAY-02/05/06 收齐（显示/隐藏/暂停 tooltip/打开设置/退出；托盘图标换猫）
- [x] SET-03～06（提醒开关/间隔 15–180/暂停；关闭设置≠退出；防抖保存）
- [x] PLAT-05 位置钳制、CFG-06 schema v2 迁移
- [x] RND-07 隐藏/待机降帧；ASSET-08 便携 `dist/PawDesk` + `tools/package.ps1`
- [x] QA 单元测试增补（interval/migrate）；手工清单见 §12.6  
**Demo**：托盘+设置完整；release 便携包可分发 — **已完成（2026-08-04）**

---

## 6. 建议执行顺序（迭代切片）

适合单人或 AI 辅助连续开发的切片：

| 迭代 | 产出 | 关键任务 |
| --- | --- | --- |
| I0 | 工程跑起来 | FOUND + 空窗 |
| I1 | 看见猫 | PLAT 透明 + RND + ASSET 单帧 + WIN |
| I2 | 拖得动、关得掉 | 拖动 + CFG 位置 + TRAY 退出 |
| I3 | 活起来 | 动画控制器 + 5 套待机 |
| I4 | 会躲会扑 | 边缘 + 鼠标互动状态机 |
| I5 | 会催你起来 | 提醒调度 + 投喂 UI |
| I6 | 能当启动器 | 快捷方式 + 径向菜单 |
| I7 | 能天天挂着 | 设置完善 + 性能 + 打包 |

---

## 7. 首期明确不做（任务禁止项）

与 prd §4.2 对齐，本计划 **不排期**：

- 多形象商城 / 付费内容
- 智能使用时长提醒模型
- 云同步与账号
- 自定义动画编辑器
- 全局快捷键配置界面
- macOS / Linux
- 社交 / 联网运营内容

出现上述需求时：先改 PRD 版本，再增补本文件新里程碑。

---

## 8. 风险与任务缓冲

| 风险 | 影响任务 | 缓冲建议 |
| --- | --- | --- |
| 透明窗黑边 / 兼容性 | PLAT-02, RND-01 | M0 必须在真机验证，不通过不进 M1 大动画 |
| 命中与穿透冲突 | PLAT-04, WIN-04 | 预留 1 迭代专攻 hit-test |
| 动画体积与内存 | ASSET-*, RND-03 | 精灵图 + 按需加载；M5 再压资源 |
| `.lnk` 解析复杂度 | SC-03 | 先 exe + 常见 lnk，失败走手动路径 |
| 多屏坐标 | PLAT-05 | M5 集中处理，M1 先单屏正确 |
| 长时间运行时钟漂移 | RM-01, PET-02 | 统一单调时钟；休眠唤醒抽测 |

---

## 9. 验收映射（任务 -> PRD）

| PRD 验收项 | 主要任务 |
| --- | --- |
| 1 透明悬浮可拖 + 配置恢复 | WIN-*, CFG-05, PLAT-02 |
| 2 多套待机动画 | PET-02/03, ASSET-03 |
| 3 边缘探头 | PET-05 |
| 4 鼠标扑近互动 | PET-06/07 |
| 5 提醒投喂闭环 | RM-*, PET-09 |
| 6 托盘能力 | TRAY-* |
| 7 快捷菜单与管理 | MENU-*, SC-*, SET-02 |
| 8 性能指标 | QA-04/05, RND-05/07 |
| 9 无账号本地闭环 | 全程无网络依赖设计 |

---

## 10. 进度跟踪（可选）

| 里程碑 | 计划开始 | 计划完成 | 实际完成 | 备注 |
| --- | --- | --- | --- | --- |
| M0 | 2026-08-01 | 2026-08-01 | 2026-08-01 | 透明窗+单帧猫+拖动+托盘退出+日志 |
| M1 | 2026-08-01 | 2026-08-01 | 2026-08-01 | 5 待机动画+配置持久化+圆形命中穿透+DPI |
| M2 | | | 2026-08-01 | 边缘探头+鼠标扑近互动+状态优先级+单元测试 |
| M3 | | | 2026-08-01 | 提醒调度+投喂闭环+托盘暂停+程序化提醒UI |
| M4 | | | 2026-08-01 | 快捷方式+启动坞菜单+设置管理+ShellExecute启动 |
| UI-P1 | 2026-08-04 | — | **阶段性完成** | 启动坞 Apple 风初版、中文对齐、HiDPI 锐化 |
| M5 | 2026-08-04 | 2026-08-04 | 2026-08-04 | 设置提醒+托盘+钳制/降帧+便携包 |
| **M6** | 2026-08-04 | — | **进行中** | bug → **钉宠 Launcher（§14）** → UI 玻璃；宠物动作/形象**最后** |

**当前建议下一动作（严格优先级）**：§13 → **§14 钉宠启动坞** 按 L0→L5 执行。

---

## 11. 已知问题与延后策略

> **产品约定（2026-08-04 起）**  
> 1. **先修 bug 与交互逻辑**  
> 2. **再修 UI（布局/命中/控件手感）**  
> 3. **宠物动作与形象优化放最后**（含飞扑重开、新 clip、形象重绘）

### 11.1 状态总览

| 项 | 状态 | 下阶段归属 |
| --- | --- | --- |
| M0–M5 功能闭环 | ✅ 可日常使用 | 维护 |
| 交互逻辑 v1 | ✅ 迟滞/驻留/保护 cute | **M6-A 继续打磨** |
| 启动坞 / 设置 UI | ✅ **Appica 精致坞 + 丝滑开合**（2026-08-07） | 微调 / 回归 |
| 提醒面板视觉 | 🟡 功能有，视觉粗 | **M6 后续** |
| 已知 bug 清单 | 见 §11.2 / §13 | 按优先级 |
| 宠物动作 / 形象 / 飞扑 | 可用底线；精修延后 | **下一步 PET-A07** |

### 11.2 已知问题 backlog（按类）

#### A. Bug / 稳定性（优先）

| ID | 现象 | 备注 |
| --- | --- | --- |
| BUG-01 | 启动报 `0x800700e8` 偶发 | 多实例/管道误报；先杀进程再启 |
| BUG-02 | 提醒窗 ↔ 待机窗切换位置/尺寸跳变、闪一下 | `app.rs` 缩放/origin |
| BUG-03 | 添加应用曾卡顿 | ✅ 异步 COM STA；不再阻塞 UI |
| BUG-03b | 点「添加应用」launcher 闪一下 | ✅ 不再切换 AlwaysOnTop；owner 绑定对话框 |
| BUG-03c | 桌面快捷方式显示不全 | ✅ Shell 虚拟桌面（用户∪公共）；去掉 `FOS_FORCEFILESYSTEM` |
| BUG-04 | 失效快捷方式体验弱 | 点进管理但未高亮该行 |
| BUG-05 | 设置页托盘暂停文案不随状态改菜单项 | 现靠 tooltip；可增强 |
| BUG-06 | 多显示器热插拔后位置 | 已有拖放/显示钳制；热插拔事件可再补 |

#### B. 交互逻辑（优先 · 含钉宠 Launcher）

| ID | 期望 | 现状 |
| --- | --- | --- |
| IX-01 | **钉宠**：开坞后宠物屏幕锚点尽量不动；卡 flip/shift | ✅ `place_launcher` |
| IX-01b | 四边四角 work area 内整卡可点 | ✅ 放置算法 + 单测 |
| IX-01c | `HiddenAtEdge` 先回可见再开坞 | ✅ snap restore |
| IX-01d | Opening/Closing 丝滑（scale+fade；宠不闪；~60fps） | ✅ design §5.6 · tech §5.4 |
| IX-02 | 列表 hover / 按压反馈（逻辑态） | ✅ hover_t / press_t 插值 |
| IX-03 | 提醒中拖动 / 点食物 / 开菜单优先级清晰 | 有规则，需手工再验 |
| IX-04 | 隐藏时提醒 pending → 显示后弹出 | M5 已做，需回归 |
| IX-05 | 点击-拖动阈值 / 关坞防抖 | 10px / 280ms 已做，可按手感微调 |
| IX-06 | 飞扑冷却与打断 | **关闭**，最后阶段再开 |

#### C. UI 修复（次优先，交互之后）

| ID | 项 | 状态 |
| --- | --- | --- |
| UI-01 | 启动坞：悬停/按压绘制、空状态与按钮节奏 | ✅ Appica primary/soft/row + chrome 动画 |
| UI-02 | 设置页：与启动坞同一套间距/字号层级 | [~] 共用 token；可再精修 |
| UI-03 | 提醒卡 / 食物按钮：对齐 design 或至少更清晰 | [ ] |
| UI-04 | 窗体切换动画（可选淡入，忌大改） | [~] 坞开合已做；提醒窗切换另议 |
| UI-05 | 快捷行图标（系统提取 ico 可选） | [ ] |

#### D. 宠物动作与形象（**最后**）

| ID | 项 | 说明 | 状态 |
| --- | --- | --- | --- |
| PET-A01 | 待机 / 状态 clip crossfade | 切换约 140ms 预乘混合 | [x] 2026-08-06 |
| PET-A01b | 待机真眨眼（非叠层黑斑） | `tools/build_idle_base.py` 虹膜 mask 眼皮 | [x] 2026-08-06 |
| PET-A01c | 拖动态持续播帧 + scale 脉冲 | 曾错误 `tick` 直接 return | [x] 2026-08-06 |
| PET-A02 | 飞扑重开与验收 | `ENABLE_MOUSE_POUNCE` | [ ] |
| PET-A03 | 新动作 / 视频抽帧补帧 | 工具链已有 | [~] 并入 PET-A07 |
| PET-A04 | 形象重绘 / 瘦版统一 | 用户拍板后再做 | [ ] |
| PET-A05 | 提醒/投喂动作精修 | 依赖形象定稿 | [ ] |
| PET-A06 | 宠物大小可调 | 设置 + 托盘；`pet.scale` 持久化 | [x] 2026-08-06 已测 |
| **PET-A07** | **动画精修** | 阶段 A：blink + stretch only | **[~] 调试中** |

#### PET-A07 — 动画精修（进行中）

**当前阶段 A（只调这两项）**

| Clip | 状态 | 说明 |
| --- | --- | --- |
| `idle_blink` | **主调** | 静态坐姿 + 真眨眼 / 轻呼吸 |
| `idle_stretch` | **主调** | 侧视朝左、横铺桌面；i2v `v3_side_left` |
| `idle_cute` / `tail_wag` / `sleep` | **停调度** | 资源保留；`IDLE_ACTION_ENABLED` 未列入 |
| watch / drag / peek / reminder | 仍加载 | 非本阶段验收重点 |

**代码开关**：`src/pet/animation.rs` → `IDLE_ACTION_ENABLED = ["idle_stretch"]`  
**间隔**：60s（本地可 `PAWDESK_CUTE_SECS=10`）  
**工具**：`gen_stretch_video.py` + `pack_stretch_from_video.py`

**阶段 B（stretch 验收后再开）**：cute / tail_wag / sleep 写回 `IDLE_ACTION_ENABLED`。

### 11.3 实现备忘（长期有效）

- 代码入口：`src/app.rs`、`src/pet/*`、`src/render/menu_ui.rs`、`src/render/text.rs`、`src/ui/radial_menu.rs`、`src/ui/tray.rs`、`src/platform/windows.rs`、`src/config/*`
- **待机**：`idle_blink` + **60s 仅 `idle_stretch`**（其它 oneshot 停）；`Watching` 不扑
- **动画工具**：stretch → `gen_stretch_video.py` / `pack_stretch_from_video.py`；warp 底 → `build_lively_pet.py`
- **显示大小**：`config.pet.scale` 默认 **0.6**；`pet_logical_size`；设置 `PetScaleDec/Inc`；托盘变大/变小；schema **v3** 迁移
- **飞扑**：`ENABLE_MOUSE_POUNCE = false`；资源与路径代码保留
- **启动坞（拍板）**：**钉宠 + Flip → Shift → Size**；半透明玻璃拟态；单 HWND = `union(pet_rect, card_rect)`；效果图 `mockups/launcher-pin-flip.*`
- **呈现**：CPU RGBA → `UpdateLayeredWindow(ULW_ALPHA)` 预乘 BGRA；禁止宠物 HWND 挂 DXGI/wgpu
- **色键**：仅品红；**禁止黑键**
- **工具**：`tools/build_lively_pet.py`（主）、`build_idle_base.py`、`build_coherent_30fps.py`（旧）、`gen_pet_videos.py`、`extract_video_frames.py`、`set_pet_from_image.py`、`package.ps1`、`xai_video_i2v.py`
- **配置坑**：勿用 PowerShell `Set-Content`/`ConvertTo-Json` 乱写 `config.json`（易 BOM/非法 JSON → 回退 bak 的旧 scale）
- **添加应用**：`shortcut/picker.rs` → Windows 原生 `IFileOpenDialog`；`build_pick_context` + 后台 STA；虚拟桌面；不切 z-order（tech §7.4）
- **构建产物**：`target/debug` 开发 · `target/release` 正式 · `dist/PawDesk` 仅 package 快照（不会随 cargo 自动更新）
- **便携包**：`tools/package.ps1` → `dist/PawDesk/`（不含 `_master` / `_video`）

### 11.3.1 动作 clip 快照（2026-08-06）

| 目录 | 用途 | 约规格 |
| --- | --- | --- |
| `idle_blink` | 默认待机（呼吸+真眨眼） | **120f @30 loop** |
| `idle_stretch` | **唯一** 60s oneshot | 72f @30 side-left（`video_stretch_v3_side_left`） |
| `idle_cute` / `tail_wag` / `sleep` | 不调度 | 资源仍在盘 |
| `idle_watch` | 中距观察 | 60f @30 loop |
| `approaching` | 飞扑（运行时关闭） | 48f 进度驱动 |
| `playing_interaction` | 扑后互动 | 30f loop |
| `dragging` / `edge_peek` | 拖动 / 边缘 | 30f loop |
| `reminder_wave` / `reminder_feed` | 提醒 / 投喂 | 30f |

### 11.4 透明底注意事项

- 品红可色键 + 边缘 flood-fill；禁止键纯黑；禁止无条件删近白像素。
- 主窗勿用 `LWA_COLORKEY` 作最终方案。

---

## 12. 进度快照（2026-08-06 · 同步版）

### 12.1 总体进度

| 范围 | 进度 | 说明 |
| --- | --- | --- |
| M0 工程骨架 | ✅ | 透明窗、托盘、日志、拖动 |
| M1 待机与配置 | ✅ | 多待机、位置持久化、DPI |
| M2 互动 | ✅（飞扑延后） | 边缘探头 + Watching |
| M3 提醒 | ✅ | 调度、投喂、暂停 |
| M4 快捷访问 | ✅ | 启动坞 + 管理 + 启动 |
| UI-P1 启动坞 | ✅ 初版 | Apple 风 + 中文 + HiDPI |
| M5 日常化 | ✅ | 设置提醒、托盘、便携包、降帧 |
| **M6 质量迭代** | 🔄 **进行中** | A bug → **B 钉宠 Launcher（§14）** → C 玻璃 UI → D 形象最后 |
| **M6 动画/缩放增量** | ✅ 2026-08-06 | 真眨眼、crossfade、拖动动画、`pet.scale` UI |
| 效果图 mockup | ✅ | `mockups/launcher-preview.*` · `launcher-pin-flip.*` |

### 12.2 已交付能力摘要

- 分层真透明桌宠 + 配置持久化 + 托盘完整菜单  
- **待机真眨眼** + **60s 仅 stretch**；距离迟滞 / Watching 驻留  
- **宠物大小可调**（设置步进 + 托盘变大/变小；默认 0.6；schema v3）  
- 提醒闭环 + 设置内开关/间隔/暂停  
- 快捷启动坞（钉宠玻璃卡）+ **原生**异步文件选择（虚拟桌面 / 不闪）  
- 隐藏不抢提醒；显示后 pending；工作区钳制  
- 便携：`dist/PawDesk/`（需 `package.ps1` 刷新）；单元测试 95 passed  
- **动画（拍板）**：**先验收 blink + stretch**；`IDLE_ACTION_ENABLED` 控制池；撒娇 1 分钟一次  
- **其后 / 并行**：钉宠 Launcher（§14）、飞扑、形象精修

### 12.3 本地路径

| 用途 | 路径 |
| --- | --- |
| 工程根 | `D:\AI练习目录\PawDesk` |
| Debug（开发） | `target\debug\pawdesk.exe` ← `cargo run` / `cargo build` |
| Release（最新优化） | `target\release\pawdesk.exe` ← `cargo build --release` |
| 便携包（可选快照） | `dist\PawDesk\` ← `tools\package.ps1`（从 release 复制，**不入库 / 不自动同步**） |
| 配置 | `%APPDATA%\PawDesk\config.json` |
| 日志 | `%LOCALAPPDATA%\PawDesk\logs\app.log` |
| 资源 | `assets\pets\cow-cat\` · `assets\tray\icon.png` |

---

## 13. 下阶段计划：M6 质量迭代

### 13.1 目标

在 **不打断日常使用** 的前提下，把产品从「能用」推到「顺手」：

1. 清掉阻塞与明显 bug  
2. **启动坞交互达标：钉宠 + Flip/Shift**（见 **§14**，本阶段主交付）  
3. 半透明玻璃 UI + 手感收敛  
4. **明确不在本阶段做**：大形象重做、新动作流水线、飞扑重开、系统真 Acrylic  

### 13.2 执行顺序（必须遵守）

```text
M6-A   改 bug / 稳定性（与坞无关的必现问题可并行）
  │
  ▼
M6-B   钉宠 Launcher 交互（§14 L0–L3）  ← 主路径
  │
  ▼
M6-C   玻璃 UI + hover/失效（§14 L4–L5）+ 设置/提醒卡
  │
  ▼
M6-D   宠物动作与形象（最后再开）
```

| 子阶段 | 内容 | 完成标志 |
| --- | --- | --- |
| **M6-A** | 修 §11.2-A；回归隐藏/提醒/异步添加 | 无必现崩溃；主路径手工过一遍 |
| **M6-B** | **§14 L0–L3**：放置算法 + 接线 + 探头预处理 + 开闭动画 | 四边四角整卡可见、宠锚点稳定 |
| **M6-C** | **§14 L4–L5** + §11.2-C：玻璃拟态、hover、失效高亮 | 观感对齐 mockup；不碰精灵大改 |
| **M6-D** | §11.2-D：动作 crossfade、飞扑、形象 | **仅当 A–C 收口后** |

### 13.3 M6-A 建议首批 bug 单

1. 提醒窗尺寸切换跳变 / 闪烁  
2. 设置打开时 origin 与完成回位是否稳定  
3. ~~异步选文件取消后置顶/焦点是否正确~~ ✅ 不再切 z-order；owner 绑定  
4. ~~桌面快捷方式显示不全~~ ✅ Shell 虚拟桌面（用户∪公共）  
5. 失效快捷方式 → 管理页可定位（可与 L5 合并）  
6. 多开启动的友好提示（可选）  

### 13.4 通用交互检查清单（手工 · 钉宠专项见 §14.6）

- [ ] 单击开坞、拖动移动、不误触  
- [ ] 关坞后 280ms 内不连环重开  
- [ ] 靠近抬头、快速掠过不抽风  
- [ ] 撒娇动画不被 Watching/贴边打断  
- [ ] 隐藏 → 提醒到期 → 显示后出现提醒  
- [ ] 提醒展示中拖动不丢状态  
- [ ] 托盘暂停/设置/退出与主窗一致  
- [ ] **钉宠**：贴右/底/角开坞，宠不「瞬移」、卡不出 work area  

### 13.5 不做清单（本阶段禁止分心）

- 重做奶牛猫全套精灵 / 大改画风  
- 默认打开飞扑  
- 开机自启（除非单独需求）  
- **系统级 Acrylic / 实时抓屏 blur**（只用半透明 + 高光拟态）  
- 径向环菜单回退  
- 未提交前的大规模目录重构  

### 13.6 文档与验收

- 每完成 §14 一个 Lx：勾任务状态 + 更新 §11.2  
- UI：对照 `mockups/launcher-pin-flip.png`；保持 DPR 1:1 锐利  
- **M6-D 启动前** 需用户确认：形象方向与是否开飞扑  

---

## 14. 钉宠 Launcher 开发计划（M6-B/C 主交付）

> **产品拍板（2026-08-04 · 精致化 2026-08-07）**  
> - 形态：方案 A 玻璃卡片坞（非径向、非强制竖向列表独占）  
> - 交互：**宠物屏幕锚点尽量钉死**；卡片 **Flip → Shift → Size**  
> - 视觉：**Appica 暖玻璃** + primary/soft/row；禁止真桌面 Acrylic / Web 组件库  
> - 动效：卡从身边长出；**宠不闪**；~**60fps**；线性 open_t + out_quint/out_cubic  
> - 单 HWND：`W = union(pet_screen_rect, card_rect)`，层内分画宠与卡  
> - 依据：`design.md` §5（v0.7）、`tech.md` §5（v0.5）；效果图 `mockups/launcher-preview.*`

### 14.1 目标与非目标

| 要做 | 不做 |
| --- | --- |
| 任意 work area 位置开坞：整卡可点 | 双窗口 / 独立 launcher 进程 |
| 贴边时宠尽量不瞬移 | 实时抓屏模糊 |
| Opening/Closing 可感知 | 重做全部精灵 |
| 半透明暖白玻璃 + 阴影高光 | 径向菜单 |
| 单元测试覆盖放置算法 | 最近使用时间（可 P2） |

### 14.2 架构要点（实现契约）

```text
capture origin / pet_screen_rect Ps（逻辑或物理统一）
        │
        ▼
place_launcher(Ps, card_size, work, dpr)
  1. flip   水平：右优先，不够则左；可选上/下
  2. shift  沿轴平移 C，使 C ⊆ work；Ps 尽量不动
  3. size   （P1）略缩 card 仍放不下时
        │
        ▼
W = union(Ps, C') (+ padding) ；必要时整体微移使 W ⊆ work
        │
        ▼
set window = W ；compose：
  pet  @ (Ps - W.origin)
  card @ (C' - W.origin)
        │
        ▼
close → restore origin（禁 persist 临时坐标）
```

| 模块 | 建议改动 |
| --- | --- |
| `src/ui/radial_menu.rs`（或拆 `launcher_place.rs`） | `place_launcher` / `LauncherPlacement` 纯函数 + 测试 |
| `src/app.rs` | `enter_menu_ui` / `exit_menu_ui` 接线；探头预处理；物理尺寸 |
| `src/render/menu_ui.rs` | 分画宠与卡；`open_t` 透明度/scale；玻璃 alpha |
| `src/pet/mod.rs` | `tick_menu_anim` 支持 Closing；可选 unhide 钩子 |
| `src/platform/windows.rs` | 可复用 work area；坞用 **fully-inside** 钳制 |

### 14.3 任务拆分（按序）

#### L0 — 放置算法（纯逻辑 · 可先合）

| ID | 任务 | 优先级 | 状态 |
| --- | --- | --- | --- |
| L0-01 | 定义 `LauncherPlacement { window, pet_local, card_local, dir, open_t 无关 }` | P0 | [x] |
| L0-02 | 实现 `place_launcher(pet_rect, card_w, card_h, gap, work, margin)`：Flip 水平 | P0 | [x] |
| L0-03 | Shift：垂直/水平平移 card，优先保持 pet 屏幕坐标 | P0 | [x] |
| L0-04 | union 窗矩形 + 必要时整体 shift（宠微移最小化） | P0 | [x] |
| L0-05 | 物理像素：输入输出与 dpr 约定（测试用 1.0/1.25/1.5） | P0 | [x] |
| L0-06 | 单元测试：中心、左/右/上/下贴边、四角、窄 work、大 dpr | P0 | [x] |

**完成标准**：无窗口 API 依赖；测试断言「card ⊆ work」「pet 位移 ≤ 阈值（理想 0，角落实底可 >0）」

#### L1 — 接线 enter/exit（主路径可用）

| ID | 任务 | 优先级 | 状态 |
| --- | --- | --- | --- |
| L1-01 | `enter_menu_ui` 调用 `place_launcher`，废弃「固定 480×320 + 粗 ox/oy」 | P0 | [x] |
| L1-02 | `resize_pet_window` 使用 placement 的 window 物理尺寸 | P0 | [x] |
| L1-03 | `compose_menu_frame` 按 `pet_local` / card 区域绘制（非死板左列） | P0 | [x] |
| L1-04 | 命中测试坐标系与 placement 一致 | P0 | [x] |
| L1-05 | `exit_menu_ui` 恢复 `overlay_origin`；不写 config 临时位 | P0 | [x] |
| L1-06 | 开坞期间禁用/忽略会改 home 的 persist | P0 | [x] |

**完成标准**：手工 §14.6 场景 1–4 通过（可无动画）

#### L2 — 边缘探头与冲突态

| ID | 任务 | 优先级 | 状态 |
| --- | --- | --- | --- |
| L2-01 | `HiddenAtEdge` 开坞前：短动画/瞬移回完全可见，再 place | P0 | [x] |
| L2-02 | 提醒进行中：保持「不可开坞」或明确排队（沿用现规则并测） | P0 | [x] |
| L2-03 | 拖动中不可开坞（已有则回归） | P0 | [x] |
| L2-04 | 多显示器：`work_area` 取宠所在屏（point/window） | P0 | [x] |

**完成标准**：探头态开坞不闪到屏外；副屏贴边正确

#### L3 — Opening / Closing 动画

| ID | 任务 | 优先级 | 状态 |
| --- | --- | --- | --- |
| L3-01 | Opening：`open_t` 驱动 card opacity + scale | P0 | [x] → 见 **L6** 升级 |
| L3-02 | 布局在 Opening 开始即用最终 placement（中途不二次 flip） | P0 | [x] |
| L3-03 | Closing 状态：反向后再缩窗回宠 | P0 | [x] → 见 **L6** |
| L3-04 | 启动成功 → 走 Closing 再 Idle（一步启动） | P0 | [x] |
| L3-05 | 关坞 280ms 防抖保持 | P0 | [x] |

**完成标准**：开/关无明显闪黑/跳尺寸；动画期间可点空白关闭可选（P1）

#### L4 — 半透明玻璃 UI

| ID | 任务 | 优先级 | 状态 |
| --- | --- | --- | --- |
| L4-01 | 卡片底：暖白 **半透明** + 内高光 | P0 | [x] → 见 **L6** Appica token |
| L4-02 | 多层软阴影对齐 mockup | P1 | [x] |
| L4-03 | 色板与 design §2 统一（深 slate primary + 粉 accent） | P1 | [x] |
| L4-04 | 禁止接入系统 Acrylic / 抓屏 blur | P0 | [x] 约束 |

**完成标准**：静态观感接近 `launcher-pin-flip` / `launcher-preview`

#### L5 — 手感与异常

| ID | 任务 | 优先级 | 状态 |
| --- | --- | --- | --- |
| L5-01 | 行 Hover / 按压填充态（MenuOpen 跟 cursor） | P1 | [x] → 见 **L6** 插值 |
| L5-02 | 失效行文案对齐 design；点击进设置并高亮 | P1 | [x] |
| L5-03 | 空列表引导（已有可微调） | P1 | [x] |
| L5-04 | Size 降级：极小 work 时缩卡高/宽（可选） | P2 | [ ] |
| L5-05 | 气泡反馈（启动失败优先） | P2 | [ ] |

#### L6 — Appica 精致化 + 丝滑防闪（2026-08-07）

| ID | 任务 | 优先级 | 状态 |
| --- | --- | --- | --- |
| L6-01 | Appica 暖色 token + primary/soft/list 控件绘制 | P0 | [x] |
| L6-02 | `menu_open_t` 线性时钟；开 380ms / 关 240ms；compose 内 out_quint/out_cubic | P0 | [x] |
| L6-03 | 宠全不透明；卡层 per-layer fade；托盘 plate 随 fade 渐入 | P0 | [x] |
| L6-04 | 子项 stagger + hover/press `approach` 插值 + press scale 0.97 | P0 | [x] |
| L6-05 | 坞打开 **60fps**（16ms）；动画 tick 内直接 `redraw` | P0 | [x] |
| L6-06 | `update_layered_rgba_ex` 原子 present；`enter_menu_ui` 立即首帧 | P0 | [x] |
| L6-07 | design v0.7 / tech v0.5 / mockup 同步 | P0 | [x] |

**完成标准**：点宠 → 卡从身边长出；宠不闪不跳不弹；动画跟手丝滑

#### L7 — 坞可用性收口（2026-08-07 晚）

| ID | 任务 | 优先级 | 状态 |
| --- | --- | --- | --- |
| L7-01 | Primary 去顶高光/底阴影（flat solid，无「两道影」） | P0 | [x] |
| L7-02 | UI 文字改 **GDI**（YaHei UI）；弃 fontdue 画启动坞字 | P0 | [x] |
| L7-03 | 列表视口 + **滚轮滚动**；软上限 128；修「只显示 2 个」 | P0 | [x] |
| L7-04 | 文档 design v0.8 / tech v0.6 / task 同步 | P0 | [x] |
| L7-05 | 添加应用：原生选择器 + 虚拟桌面 + 不闪；docs → design v0.9 / tech v0.7 | P0 | [x] |
| L7-05 | 更精致字体（ClearType/子像素/字重体系） | P2 | [ ] **后期** |

**完成标准**：3+ 应用可全见（≤4 直接见，更多滚轮）；主按钮无白条；拉丁/中文基线齐

### 14.4 建议迭代切片（单人 / AI 辅助）

| 切片 | 产出 | 任务 |
| --- | --- | --- |
| **S1** | 算法绿 | L0 全做完，`cargo test` |
| **S2** | 边角可用 | L1 + L2，手工四角 |
| **S3** | 动效顺 | L3 |
| **S4** | 好看能用 | L4 + L5-01/02 |

依赖：S1 无 UI 阻塞；S2 依赖 S1；S3/S4 可部分并行但优先 S2。

### 14.5 风险

| 风险 | 缓解 |
| --- | --- |
| 动态窗尺寸 + layered 闪烁 | Closing 完成后再 `resize` 回 128；与提醒窗跳变同一类修法 |
| 命中/穿透与大透明 union | 仅 card+pet 实体接收点击；空白可关坞或穿透策略写清 |
| dpr 非整数 | 与现 `Dpi::new` snap 一致；placement 全程物理像素 |
| 与 settings/reminder 叠态 | 互斥保持：reminder > menu；menu→settings 关坞 |

### 14.6 钉宠验收清单（手工）

- [ ] **中心**：卡在宠右侧（或空侧），宠不位移  
- [ ] **贴右**：卡 flip 到左，宠仍近右边  
- [ ] **贴左**：卡在右  
- [ ] **贴底**：卡 shift 上移，不进任务栏（work area）  
- [ ] **贴顶**：卡在下或 shift  
- [ ] **右下角 / 左上角**：整卡可见；宠位移最小  
- [ ] **探头隐藏**：先露出再开坞  
- [ ] **关坞**：回 origin；config 位置未污染  
- [ ] **启动项**：启动后坞关闭  
- [ ] **125% / 150% DPI**：无裁切、字清晰  
- [ ] **副屏**（若有）：在宠所在屏 work 内  
- [ ] **丝滑开坞**：宠不闪；卡从身边长出（无弹跳/空帧）  
- [ ] **60fps 观感**：开合与 hover 无明显跳帧  
- [ ] **控件**：flat primary（无白条）/ soft / 列表 hover·press  
- [ ] **列表**：≥3 应用全可见；多应用可滚轮；提示「共 N 个」  
- [ ] **文字**：GDI 基线齐（精致化后期）  

### 14.7 进度表

| 切片 | 状态 | 完成日 |
| --- | --- | --- |
| S1 L0 算法 | [x] | 2026-08-04 |
| S2 L1+L2 接线 | [x] | 2026-08-04 |
| S3 L3 动画 | [x] | 2026-08-04 |
| S4 L4+L5 UI | [x] | 2026-08-04 |
| S5 L6 精致化+丝滑防闪 | [x] | 2026-08-07 |
| **S6 L7 可用性收口（字/列表/主按钮）** | **[x]** | **2026-08-07** |

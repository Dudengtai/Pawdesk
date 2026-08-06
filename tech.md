# PawDesk 技术设计文档

| 项目 | 内容 |
| --- | --- |
| 版本 | **v0.4**（2026-08-06：待机眨眼管线 + pet.scale UI） |
| 依据 | `prd.md` v0.5 · `design.md` v0.5 |
| 排期 | `task.md` |
| 环境 | `env.md` |

---

## 阅读导航

| 想了解… | 看 |
| --- | --- |
| 整体架构与目录 | [§1](#1-架构总览) · [§2](#2-源码模块地图) |
| 宠物状态机 / 动画 | [§3](#3-宠物领域) |
| 透明窗 / 呈现 | [§4](#4-窗口与呈现) |
| 启动坞钉宠算法 | [§5](#5-快捷启动坞) |
| 提醒调度 | [§6](#6-健康提醒) |
| 快捷方式 / 配置 | [§7](#7-快捷方式与配置) |
| 性能 / 测试 / 发布 | [§8](#8-性能错误与发布) |
| 风险与验收 | [§9](#9-风险与技术验收) |

**实现原则（一句话）**

- 业务状态机集中在 `pet`；UI 不直改状态。  
- 主线程不阻塞 I/O；配置防抖写盘。  
- 宠物 HWND：**CPU RGBA + `UpdateLayeredWindow`**，不挂 DXGI/wgpu 交换链。  
- 启动坞：**钉宠 + Flip/Shift**，单窗 union(宠, 卡)。

---

## 1. 架构总览

### 1.1 要解决的技术问题

1. 无边框、真透明、置顶、可拖动的桌面窗。  
2. 低占用下播帧动画 + 定时任务。  
3. 待机 / 互动 / 提醒 / 菜单互斥的状态机。  
4. 快捷方式增删排序启动 + 本地配置。  
5. 文件选择、启动进程不堵主循环。

### 1.2 技术栈

| 层级 | 选型 | 职责 |
| --- | --- | --- |
| 语言 | Rust 2021 | 业务与状态 |
| 窗口 | `winit` | 事件循环、窗口 |
| 平台 | `windows` crate | 工作区、分层窗、Shell 启动等 |
| 呈现 | CPU 位图 + `UpdateLayeredWindow` | 宠物与叠层 UI 真透明 |
| 自绘 UI | `fontdue` + 自写 compose | 启动坞 / 设置 / 提醒卡 |
| 托盘 | `tray-icon` | 托盘与菜单 |
| 序列化 | `serde` + JSON | 配置 |
| 日志 | `tracing` | 文件日志 |
| 文件框 | `rfd`（工作线程） | 选 exe/lnk |

说明：仓库内保留 `wgpu` 相关代码，**当前宠物主路径不走 GPU 表面**，避免与 layered 冲突。

### 1.3 线程模型

```text
主线程：事件循环 · 状态机 · 动画时钟 · 合成 present · 托盘命令
后台：  配置写盘（防抖）· 异步文件选择 · 进程启动校验
```

- 后台只通过通道 / 事件回主线程，不直接改 UI 状态。  
- 用户操作进入 `AppEvent` / 状态机，避免隐式交叉修改。

---

## 2. 源码模块地图

```text
src/
├─ main.rs              入口、日志
├─ app.rs               生命周期、叠层 UI 切换、开坞接线
├─ error.rs · event.rs
├─ config/              AppConfig、原子写、防抖
├─ pet/                 状态机 · 动画 · 移动 · 互动
├─ reminder/            调度 · 文案
├─ shortcut/            模型 · 仓库 · 启动 · 选择器
├─ render/              menu_ui · reminder_ui · text · easing
├─ ui/                  radial_menu · launcher_place · tray · pet_window
└─ platform/windows.rs  分层窗、工作区、DPI、启动

assets/pets/cow-cat/    分 clip 帧序列 + meta.json
assets/tray/            托盘图标
tools/                  抽帧、打包、视频工具
dist/PawDesk/           便携包（package.ps1）
```

| 模块 | 职责 |
| --- | --- |
| `app` | 窗尺寸/位置切换（宠 / 坞 / 提醒 / 设置）、事件汇总 |
| `pet` | `PetState`、clip 切换、拖动/边缘/菜单动画 t |
| `launcher_place` | **纯函数** `place_launcher`（物理像素） |
| `radial_menu` | 条目布局 `layout_pinned`、命中 |
| `menu_ui` | 玻璃坞 + 设置面板 CPU 绘制 |
| `reminder` | 间隔、暂停、补发一次 |
| `shortcut` | 列表持久化、`ShellExecute` 类启动 |
| `platform` | `UpdateLayeredWindow`、work area、钳制 |

---

## 3. 宠物领域

### 3.1 状态机

```text
Idle ──30s──> Idle(one-shot) ──完──> Idle
Idle ──中/近距──> Watching ──远──> Idle
（关闭）Watching ──飞扑──> Approaching ──> Playing ──> Idle
* ──提醒──> Reminder(*) ──回位──> Idle
Idle/Watching/Edge ──单击──> MenuOpen ──关──> Idle
* ──拖──> Dragging ──放──> Idle
Idle ──贴边──> HiddenAtEdge ──点/恢复──> Idle
```

优先级（高 → 低）：**Dragging &gt; Reminder &gt; MenuOpen &gt; Edge &gt; Playing &gt; Approaching &gt; Watching &gt; Idle**。

规则：

- 拖动打断提醒 → 结束后再处理 pending。  
- 提醒中不进菜单；菜单中不自动扑近。  
- 开坞前若 `HiddenAtEdge`：`snap_restore_from_edge` 再 place。

### 3.2 动画资源

路径：`assets/pets/cow-cat/<clip>/`（PNG + `meta.json`，常见 256 帧图）。

| Clip | 用途 | 备注 |
| --- | --- | --- |
| `idle_blink` | 默认待机（循环） | **真眼皮替换**（虹膜 mask）；工具 `tools/build_idle_base.py` |
| `idle_stretch` / `cute` / `tail_wag` / `sleep` | 30s 撒娇 one-shot | 身份一致 warp 序列见 `build_coherent_30fps.py` |
| `idle_watch` | Watching | 无缝循环；进/出与坐姿对齐 |
| `approaching` / `playing_interaction` | 飞扑链路（**运行关闭**） | |
| `dragging` / `edge_peek` | 拖动 / 边缘 | 拖动态 **持续播帧** |
| `reminder_wave` / `reminder_feed` | 提醒 / 投喂 | |

**待机规则**

- Base：`idle_blink`（约 4s @30fps 循环；静坐 + 自然眨眼，忌身体大幅 morph 当第二层动画）。  
- 墙钟约 30s 随机 one-shot；**Watching 不重置** 30s 计时。  
- one-shot 播放至少约 ≥2.8s 可感知；末帧短 hold 再回 base。  
- clip 切换：`begin_crossfade` ≈ **140ms** 预乘 alpha 混合。  
- 呈现：`display_frame_f` 亚帧混合 + 约 30fps 刷新密集 clip。  

**下一步（工程）**：优化 one-shot 池资源与观感（`idle_stretch` / `cute` / `tail_wag` / `sleep`）— 任务 **PET-A07**（`task.md`）。优先 `tools/build_coherent_30fps.py` 分动作精修，保持单身份与脚底锚点。

**飞扑**

- `ENABLE_MOUSE_POUNCE = false`：近距只 Watching。  
- 资源与路径代码保留，后期开开关。

### 3.3 移动

`movement`：去光标（抛物线，飞扑用）、回家、去屏幕中心、边缘 hide/restore。缓动见 design `ease.smooth`。

### 3.4 显示缩放（`pet.scale`）

| 项 | 说明 |
| --- | --- |
| 配置 | `config.pet.scale`（`AppConfig` / `%APPDATA%/PawDesk/config.json`） |
| 基准 | `PET_WINDOW_SIZE = 128` 逻辑 px |
| 实际边长 | `pet_logical_size(scale)` = round(128 × scale)，钳位约 64–256 |
| 默认 | **0.6**（schema v3 迁移会把旧 `1.0` 拉到默认） |
| 范围 / 步进 | **0.5–1.5** / **0.1**（`clamp_pet_scale` / `step_pet_scale`） |
| 入口 | 设置页 `−`/`+`；托盘 `PetScaleUp` / `PetScaleDown` |
| 生效 | 待机即时 `resize_pet_window`；设置内改配置，退出设置回宠窗时用新尺寸 |
| 建窗 | `create_window` 用 `pet_size()`，并强制物理 resize（防 DPI 忽略 LogicalSize） |

---

## 4. 窗口与呈现

### 4.1 窗口属性

- 无边框、置顶、不抢焦点。  
- 关窗 ≠ 退出（退出走托盘）。  
- 位置：工作区坐标；多屏用宠所在屏 `work_area`；显示器变化时钳制。  
- 宠窗逻辑边长随 `pet.scale` 变化（非写死 128）。

### 4.2 呈现路径（关键）

```text
业务合成 RGBA（逻辑布局 × DPR → 物理缓冲）
        ↓
UpdateLayeredWindow + 预乘 BGRA + ULW_ALPHA
```

- **禁止**在宠物 HWND 上挂 DXGI/wgpu surface。  
- 透明命中：按 alpha / 实体区；勿对整窗永久 `WS_EX_TRANSPARENT`。  
- 色键：资源处理**仅品红**；禁止黑键（伤黑毛）。

### 4.3 DPI

- 布局用逻辑像素（96 DPI 基准）。  
- 合成与窗尺寸：`logical × snap_dpr` → 物理。  
- 叠层 UI 尽量 1:1 物理像素绘制，避免模糊放大。

### 4.4 叠层模式（同一 HWND）

| 模式 | 大致尺寸 | 说明 |
| --- | --- | --- |
| 宠物 | 128×`pet.scale` 逻辑（默认 ~77） | 日常；用户可调 |
| 启动坞 | union(宠, 卡) | `place_launcher` |
| 提醒 | 固定提醒窗 | 居中 |
| 设置 | ~420×580 逻辑 | 居中 |

`overlay_origin`：进叠层前宠物 top-left；退出恢复；**禁止**把坞临时坐标写入配置。

---

## 5. 快捷启动坞

### 5.1 产品契约（对照 PRD）

- 单击开坞；宠物为**屏幕锚点**。  
- 卡片为玻璃浮层；**非**径向环。  
- 任意 work area 位置：整卡可点、尽量不裁切。

### 5.2 放置算法 `place_launcher`（物理像素）

模块：`src/ui/launcher_place.rs`。

```text
输入：pet 屏矩形、card 宽高、gap、work、margin
  1. Flip   水平优先右，不够则左（必要时上/下）
  2. Shift  只动 card 进 work
  3. Union  W = union(pet, card) + padding（padding 不挤出 work）
  4. 仅当 content 仍越界 → 整体微移（记录 pet_screen_delta）
输出：window / pet_local / card_local / dir
```

辅助：`logical_to_physical` / `physical_to_logical` / `snap_dpr`。

### 5.3 接线与绘制

| 步骤 | 位置 |
| --- | --- |
| 开坞 | `app::enter_menu_ui`：探头 snap → place → resize + 定位 |
| 布局 | `layout_pinned`：条目在 **card** 内；宠在 **pet_local** |
| 绘制 | `compose_menu_frame`：仅 card 区玻璃；宠精灵；hover/失效态 |
| 动画 | `menu_open_t`：开 ~250ms snappy；关 ~180ms smooth；几何锁定 |
| 关坞 | Closing 完 → `restore_overlay_origin`；280ms 防连点 |
| 多屏 | `work_area_from_point(宠中心)` |

卡片逻辑尺寸常量：`CARD_LOGICAL_W/H`（约 360×300）。  
设置：失效项可 `settings_highlight_row` 高亮列表行。

### 5.4 不做

- 系统 Acrylic / 实时抓屏 blur（半透明色 + 高光拟态即可）。  
- 双进程独立启动器窗口。

---

## 6. 健康提醒

### 6.1 调度

- 默认间隔 60 分钟（设置 15–180）。  
- 单调时钟；暂停整周期不累计误触。  
- 启动补发：**最多一次**，不连弹。  
- `last_completed_at` 写配置。

### 6.2 流程

```text
Due → 存原位 → 移中央 → Showing（文案+食物）
    → 点食物 Feeding → 回原位 Returning → Idle
```

拖动中 due → pending；松手后再进。隐藏宠物时 due 可 pending，显示后再出。

---

## 7. 快捷方式与配置

### 7.1 ShortcutItem

字段：`id (Uuid)` · `name` · `target_path` · `arguments` · `working_directory` · `icon_path` · `sort_order` · `enabled`。

- 启动前校验路径；失效保留 + UI 提示。  
- 删除只删配置条目。  
- 启动：安全 API，禁止危险 shell 拼接。

### 7.2 配置

路径：`%APPDATA%/PawDesk/config.json`（以 env 为准）。

- `schema_version` 当前 **3** + `migrate_config`。  
  - v2→v3：将 `pet.scale` 置为产品默认 **0.6**（修正历史写死 1.0 过大）。  
- 主要字段：`pet.scale` / `pet.opacity` / `pet.edge_hide_enabled` · `reminder.*` · `shortcuts[]` · `window.x/y`。  
- 加载：去 UTF-8 BOM；主配置失败则读 `.bak` 并**回写主文件**；启动 load/migrate 后可立刻 save 固化迁移。  
- 原子写 + `.bak`。  
- 防抖：拖动位置、排序、scale 等。

### 7.3 托盘命令

`TrayCommand`：`ShowPet` · `HidePet` · `PetScaleUp` · `PetScaleDown` · `ToggleReminderPause` · `OpenSettings` · `Exit`。

### 7.4 日志

`%LOCALAPPDATA%/PawDesk/logs/` · `tracing`。  
启动应打：`pet_scale`、`pet_logical`、建窗 `logical`/`phys_w`（便于核对缩放是否生效）。

---

## 8. 性能、错误与发布

### 8.1 性能策略

| 状态 | 建议 |
| --- | --- |
| 待机（密帧 blink） | 约 **30 FPS** 呈现亚帧混合；隐藏可再降 |
| 菜单/移动/提醒 | 临时提帧 |
| 目标 | 待机 CPU &lt; 3%、内存 &lt; 200MB（PRD） |

### 8.2 错误

- 统一 `AppError`；用户可读 + 日志细节。  
- 启动失败、选文件取消、配置损坏：不崩，可回退默认/备份。

### 8.3 发布

- `tools/package.ps1` → `dist/PawDesk/`（exe + assets）。  
- 无管理员常规运行；资源相对 exe。

### 8.4 测试

- 单元：状态机、调度、配置、`clamp_pet_scale` / `step_pet_scale`、排序、**place_launcher**、菜单布局。  
- 手工：四边四角开坞、探头开坞、提醒冲突、托盘、DPI 缩放、**宠物变大/变小与设置百分比**、待机眨眼观感。

---

## 9. 风险与技术验收

### 9.1 风险

| 风险 | 应对 |
| --- | --- |
| 透明窗黑边 / 兼容 | 真机验 layered；禁用错误色键 |
| 命中与穿透 | 实体 alpha；叠层全捕获点击 |
| 动画体积 | 分 clip、按需加载 |
| 多屏坐标 | work area + 钳制 + 托盘找回 |
| 长时间时钟 | 单调时钟；休眠后抽测 |

### 9.2 技术验收（对照 PRD §7）

1. 透明可拖宠物 + 配置恢复。  
2. 多待机 / 30s 撒娇。  
3. 边缘探头。  
4. 鼠标观察（飞扑可选关）。  
5. 提醒投喂闭环。  
6. **钉宠启动坞** 四边四角整卡在 work 内。  
7. 快捷方式 CRUD + 启动 + 失效提示。  
8. 托盘完整。  
9. 性能量级达标。  
10. 核心单测通过。

---

## 10. 版本记录

| 版本 | 日期 | 说明 |
| --- | --- | --- |
| v0.1 | 2026-08-01 | 初稿 |
| v0.2 | 2026-08-04 | 钉宠 §7.3、呈现路径 |
| **v0.3** | **2026-08-04** | **按模块重排**；阅读导航；与 PRD v0.4 / 钉宠实现同步 |

# PawDesk 技术设计文档

| 项目 | 内容 |
| --- | --- |
| 版本 | **v0.25**（2026-08-18：提醒旅途叠层内轻跃） |
| 依据 | `prd.md` v0.7.12 · `design.md` v0.27 |
| 排期 | `task.md` |
| 环境 | `env.md` |
| 文档目录 | 本文件与其它规格均在仓库 `docs/` 下（见 `docs/README.md`） |

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
- 启动坞：**钉宠 + Flip/Shift**，单窗 union(宠, 卡)；开合动画几何锁定；**关坞保留坞尺寸 HWND，开/关只换位图**（改尺寸会丢掉 layered 位图）。

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
| 自绘 UI | CPU compose + **GDI 文字** | 启动坞 / 设置 / 提醒卡（字：`CreateFontW`/`DrawTextW`） |
| 托盘 | `tray-icon` | 托盘与菜单 |
| 序列化 | `serde` + JSON | 配置 |
| 日志 | `tracing` | 文件日志 |
| 文件框 | Windows：`IFileOpenDialog`（工作线程）；其它：`rfd` | 选 exe/lnk/url |

说明：仓库内保留 `wgpu` 相关代码，**当前宠物主路径不走 GPU 表面**，避免与 layered 冲突。

### 1.3 线程模型

```text
主线程：事件循环 · 状态机 · 动画时钟 · 合成 present · 托盘命令
后台：  配置写盘（防抖）· 异步文件选择 · 进程启动校验
钩子：  启动坞打开期间：WH_MOUSE_LL 专用线程（自带消息泵）→ 原子标志
```

- 后台只通过通道 / 事件回主线程，不直接改 UI 状态。  
- **启动坞窗外点击**（F-SC-10）：`WH_MOUSE_LL` 必须装在**专用空闲线程**（自带 GetMessage 消息泵），winit 渲染线程只每帧轮询原子标志。**禁止把低层钩子装到渲染线程**：每次系统鼠标事件（含移动）都会同步派发到安装线程，渲染忙时全系统鼠标输入被卡住（已踩坑：开坞后鼠标移动卡顿）。  
- 用户操作进入 `AppEvent` / 状态机，避免隐式交叉修改。

---

## 2. 源码模块地图

```text
README.md               工程入口说明 → 指向 docs/
docs/                   全部项目文档（prd/tech/design/task/env + mockups）
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
tools/                  抽帧、打包、安装包（package.ps1 / make-installer.ps1）
dist/                   便携包 + Setup.exe（不入库）
```

| 模块 | 职责 |
| --- | --- |
| `app` | 窗尺寸/位置切换（宠 / 坞 / 提醒 / 设置）、事件汇总 |
| `pet` | `PetState`、clip 切换、拖动/边缘/菜单动画 t |
| `launcher_place` | **纯函数** `place_launcher` / `place_settings_near_point` / `settings_rect_at`（物理像素） |
| `radial_menu` | 条目布局 `layout_pinned`、命中 |
| `menu_ui` | 玻璃坞 + 设置面板 CPU 绘制 |
| `reminder` | 间隔、暂停、补发一次 |
| `shortcut` | 列表持久化、`ShellExecute` 类启动 |
| `platform` | `UpdateLayeredWindow`、work area、钳制 |

---

## 3. 宠物领域

### 3.1 状态机

```text
Idle ──60s──> Idle(one-shot) ──完──> Idle
Idle ──中/近距──> Watching ──远──> Idle
* ──提醒──> Reminder(*) ──回位──> Idle
Idle/Watching/Edge ──单击──> MenuOpen ──关──> Idle
* ──拖──> Dragging ──放──> Idle
Idle ──贴边──> HiddenAtEdge ──点/恢复──> Idle
```

优先级（高 → 低）：**Dragging &gt; Reminder &gt; MenuOpen &gt; Edge &gt; Watching &gt; Idle**。

规则：

- 拖动打断提醒 → 结束后再处理 pending。  
- 提醒中不进菜单。  
- 开坞前若 `HiddenAtEdge`：`snap_restore_from_edge` 再 place。

### 3.2 动画资源

路径：`assets/pets/cow-cat/<clip>/`（PNG + `meta.json`，常见 256 帧图）。

| Clip | 用途 | 备注 |
| --- | --- | --- |
| `idle_blink` | 默认待机 | 正面母版坐姿；开 / 半闭 / 闭 3 帧 hold |
| `look_yaw` / `look_pitch` / `look_diag` | 头跟随 | 姿态条；运行时只用手写关键帧 + 身体锁定（预乘混合）；虹膜另叠。`look_pitch` 由母版品红静帧软抠重画（`tools/pack_pitch_from_gen.py`） |
| `idle_yawn` | 60s one-shot | ~77f @30；母版脸哈欠；峰值画气泡「困死我了…」 |
| `idle_stretch` | 60s one-shot | 110f @50（2.20s，压过 `ACTION_MIN_SECS=2.2`）；7 档 sit→转→下蹲→探→半峰→峰→倒放；峰值眯眼吐舌；无气泡、不拓窗 |
| `idle_groom` | 60s one-shot | 110f @50（2.20s）；sit→抬左爪→舔→擦颊→擦耳→再舔→回落→sit；无气泡、不拓窗 |
| `reminder_hop` | **下盘不用** | 旧小窗 hop clip 仍在磁盘；旅途改为叠层内 sit+squash，运行时不再切换此 clip |

**待机规则**

- Base：`idle_blink`。随机 **2.8–6.2s** 眨一次（约 200ms；约 18% 双眨）。  
- 头眼：`src/pet/look.rs`。瞳孔先跟（~75ms），头后跟（~155ms）。`look_yaw` 13 帧条带只取偶数关键帧（光流补帧会抖）。下半身锁 `idle_blink/000`（预乘 lerp，避免交界发黑）。死区外才换姿态条。眨眼时保持转头，不弹回正面。禁止整图镜像。  
- 墙钟约 **60s** 播 `IDLE_ACTION_ENABLED`（**`idle_yawn` + `idle_stretch` + `idle_groom` 轮播**）。本地可 `PAWDESK_CUTE_SECS`。Watching / 躲边不饿死计时；躲边到点先 restore。oneshot 中停转头/眨眼。  
- 哈欠：`tools/pack_idle_yawn.py` 把五官贴回母版坐姿。进场帧 0 书挡；出场 `go_idle_with_settle`。峰值 `src/render/yawn_bubble.rs` 画漫画气泡；窗向右扩（原点不动），贴右屏才 flip 左。  
- 哈欠呈现：overlay 里的猫必须先走待机同一套 `scale_rgba_centered` letterbox（左右/顶/爪边距），再 `compose_yawn_frame`。禁止把 256 图铺满 `pet_phys`（进出哈欠会突然放大/缩小）。书挡帧与 `idle_blink/000` 像素一致。  
- 伸懒腰：`tools/pack_idle_stretch.py`。母版 edit-chain 5 键 + sit 书挡 hold；回程倒放去程。帧 0 / 末 sit ≡ `idle_blink/000`。**不拓窗、无气泡**，走普通 letterbox。  
- 伸懒腰打包：源图 1024 品红软抠 → 去暗晕 → 预乘缩到 256 → 轮廓去品红边。禁止先缩再抠（中间帧会发虚）。禁止整段 i2v 抽帧进盘；禁止把坐姿整脸贴到已离位的头上（会双头）。键须保住母版墨线粗细与毛发排线/网点。  
- 伸懒腰表情：坐/转/下蹲睁眼；探轻笑；半峰开始眯；峰值开心眯眼 + 张嘴吐舌（参考 `_master/stretch_expr_ref.png`）。  
- 舔爪洗脸：`tools/pack_idle_groom.py`。母版 edit-chain 4 键（抬爪 / 舔 / 擦颊 / 擦耳）+ sit 书挡 hold；回落复用抬爪，再舔复用舔，**不倒放整段**。帧 0 / 末 sit ≡ `idle_blink/000`。只抬观众左侧前爪；舔键闭眼 + 小粉舌贴爪垫；擦脸舌收回。**不拓窗、无气泡**，走普通 letterbox。  
- 舔爪洗脸打包：源图品红软抠 → 去暗晕 → 预乘缩到 256 → 轮廓去品红边。禁止整段 i2v 抽帧进盘；禁止把坐姿整脸贴到已离位的头上。  
- 播帧：最近邻，禁亚帧混合。oneshot 回 base 约 100ms 书挡。  
- 呈现：密集 clip **本帧直接 present**。缩放预乘双线性。  
- 加载：`AnimationLibrary` 只读 `idle_blink` / look 三带 / `idle_yawn` / **`idle_stretch`** / **`idle_groom`**（`reminder_hop` 可仍在磁盘，运行时不切）。拖动 / 贴边 / 提醒旅途与卡片到位均保持母版坐姿。旧写实猫 clip 与飞扑路径已下盘。

### 3.3 移动

`movement`：提醒旅途在**叠层局部槽**里 hop（HWND 钉死）；边缘 hide/restore 仍滑窗。缓动见 design `ease.smooth` / `ease_in_out_cubic`。

### 3.4 显示缩放（`pet.scale`）

| 项 | 说明 |
| --- | --- |
| 配置 | `config.pet.scale`（`AppConfig` / `%APPDATA%/PawDesk/config.json`） |
| 基准 | `PET_WINDOW_SIZE = 128` 逻辑 px |
| 实际边长 | `pet_logical_size(scale)` = round(128 × scale)，钳在 `PET_SCALE_MIN/MAX`（约 64–128） |
| 默认 | **0.6**（schema v3 迁移会把旧 `1.0` 拉到默认） |
| 范围 / 步进 | **0.5–1.0** / **0.1**（`clamp_pet_scale` / `step_pet_scale`；设置与托盘共用） |
| 入口 | 设置页 `−`/`+`（`pet_scale_draft` 预览，「完成」写入）；托盘 `PetScaleUp` / `PetScaleDown` |
| 生效 | 待机即时改窗 + `idle_present_pos` 原子 ULW。开坞（`enter_menu_ui`）与进设置（`begin_settings_from_launcher`）先跑 `overlay_pad_for_max_pet`：按 `PET_SCALE_MAX` 预留叠层，卡片/当前宠的**屏幕**矩形不动，只把 origin 往宠外侧挪并 `RadialLayout::translate`。`±` 仍只走 `sync_pet_slot_to_scale`（卡与 present 钉死）。旧配置 `>1.0` 启动时钳回 1.0。「完成」写 scale + `overlay_origin`；`grow_overlay_to_contain_pet_slot` 兜底扩画布（只长大）；`sync_dock_hwnd_slot_from_layout` 抄槽且 **HWND 只允许变大**（缩窗会丢掉 layered 位图）。Esc 回进入时 snapshot（已是预留后的画布）。 |
| 建窗 | `create_window` 用 `pet_size()`，并强制物理 resize（防 DPI 忽略 LogicalSize） |

---

## 4. 窗口与呈现

### 4.1 窗口属性

- 无边框、置顶、不抢焦点。  
- **Release 无控制台**（`windows_subsystem = "windows"`）；debug `cargo run` 仍留控制台。启动失败 Release 弹系统 MessageBox，日志仍写 `%LOCALAPPDATA%/PawDesk/logs/app.log`。  
- 宠 HWND：`WS_EX_TOOLWINDOW` + winit `skip_taskbar`，不进任务栏 / Alt-Tab。托盘是唯一程序入口。  
- 关窗 ≠ 退出（只藏猫；退出走托盘）。  
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
| 宠物（未开过坞） | 128×`pet.scale` 逻辑（默认 ~77） | 日常；用户可调 |
| 宠物（开过坞之后） | **保持**上次 `place_launcher` union 尺寸 | `dock_hwnd`：宠画在槽位，空白透明穿透；**不再缩回 128** |
| 启动坞 | union(宠, 卡) | `place_launcher`；尺寸已与待机相同则只 ULW |
| 提醒 | 固定提醒窗 | 居中；进提醒清 `dock_hwnd` |
| 设置 | 启动坞卡内滑入 | 仅肉垫印入口；窗几何锁在坞上 |

`overlay_origin`：进叠层前**宠物桌面点**（`pet_desk_origin` = `dock_hwnd.pos` + 宠槽，不是 HWND 原点）；退出恢复；**禁止**把坞临时坐标写入配置。持久化位置同样写宠桌面点。

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

辅助：`logical_to_physical` / `physical_to_logical` / `snap_dpr` / `union_rects`。

**设置停靠（同文件）**

| 函数 | 作用 |
| --- | --- |
| `place_settings_near_point(anchor, w, h, work, margin)` | 以锚点为中心种子，`shift` 钳进 work，得到最终设置矩形 |
| `settings_rect_at(anchor, final, w, h, t)` | 转场中插值：中心 anchor→final，scale **0.90→1**（`ease_out_quint`）；`t=1` ≡ `final` |

### 5.3 接线与绘制

| 步骤 | 位置 |
| --- | --- |
| 开坞 | `app::enter_menu_ui`：探头 snap（只写 `overlay_origin`，不挪 HWND）→ `place_launcher` → `menu_list_scroll=0` → `layout_pinned_scroll` → 先栅格卡缓存 → 尺寸已是坞则**只 ULW**，否则 `sync_layered_hwnd` 一次长大 → `remember_dock_hwnd` |
| 布局 | `layout_pinned_scroll(entries, …, list_scroll)`：chrome + 固定「最近启用」框 +「应用列表」标题 + **视口内**快捷行；宠在 **pet_local** |
| 列表数据 | `build_entries`：`rank_frequent` 最多 6 个 `Recent` + 全部 **enabled** 快捷方式，`.take(MAX_SHORTCUTS=128)` 仅软上限；常用条不计入 `list_total` |
| 滚动 | `app::scroll_menu_list` ← `WindowEvent::MouseWheel`；`clamp_list_scroll`（转场中 / 开合动画中禁用） |
| 绘制 | 开合：`present_menu_cached` 对静帧卡层 scale+fade + 钉宠（宠矩形**不**参与 scale）；落定后 `compose_menu_frame` live（hover/press） |
| 文字 | `render/text.rs`：**GDI** 白底黑字 → 覆盖率 → 着色（YaHei UI Medium）；开合不重跑 GDI |
| 动画时钟 | `pet::tick_menu_anim`：`menu_open_t` **视觉** 0..1（ease-out 两端）；开 **180ms** / 关 **140ms** |
| 视觉曲线 | `menu_visual_scale` 0.95→1 绕宠心；`menu_visual_fade` ×1.22 领先；无 stagger；坞卡无外投影，顶高光 `fill_top_sheen` 裁进圆角 |
| 交互 chrome | `MenuChromeState { hover, press, hover_t, press_t, drag, drag_draft, rows_blank }`；`app` 每帧 `approach` 插值 |
| 列表拖动 | `ui/list_drag.rs`：400ms 长按 + 8px slop → `Dragging`；插入下标按指针 y；空碗命中删条目 |
| 拖动合成 | 按下即 `prerender_drag_images`（拎起行无投影 / 碗 / 文案）+ `prerender_list_rows` + 空白卡层；`DragLayersKey` 只在 scroll/总数/hover/press/say 变时重建；每帧 `present_menu_drag` 只 blit 行到 `drag_slot_y` + 拎起行 + 碗 |
| 肉垫印→设置 | `handle_menu_entry` → `begin_settings_from_launcher`（见 §5.4）；**唯一**设置入口 |
| 设置「暂停」 | 不写 `reminder.paused`；`menu_say = SAY_NO_PAUSE` + `pet.begin_sly_pause`（`sly_pause` 1 帧，只换脸，~2.8s） |
| 关坞 | Closing 完 → `restore_overlay_origin_window`：有 `dock_hwnd` 则**不缩窗**，把窗原点钉回宠槽对桌面点，`compose_pet_in_slot` 只画宠；无 `dock_hwnd` 才走 `begin_idle_present_at` 缩回 128；280ms 防连点 |
| 窗外点击 | `platform::OutsideClickGuard`：`WH_MOUSE_LL` 专用钩子线程 + 原子标志（不吞点击）；`enter_menu_ui` 装、`about_to_wait` 每帧 `take_outside_click()` → 窗外左键即 `exit_menu_ui`；每帧同步窗口物理矩形；设置转场 340ms 内忽略；`menu_ui_active` 置 false 的各路径卸载（关坞 / 设置转场落定） |
| 多屏 | `work_area_from_point(宠中心)` |

卡片：`CARD_LOGICAL_W/H` ≈ **360×450**；`LIST_VISIBLE_ROWS=5`；「再叼一个」与列表之间固定「最近启用」分区（标题 + 整宽圆角框，框内最多 **6** 个图标；`launch_count` 降序，并列 `last_launched_at_ms` 新的在前；空框仍保留）；列表前有「应用列表」标题与之隔离；右上角肉垫印进设置；**禁止**把产品做成「最多 5 个就装不下」。  
设置卡只含健康提醒 + 宠物大小；快捷方式增删排序在坞内完成。失效行点开文件框换路径。

### 5.4 呈现与防闪（重要）

| 点 | 实现 |
| --- | --- |
| 原子 present | `platform::update_layered_rgba_ex(w,h,rgba, Some((x,y)))`：同一次 `UpdateLayeredWindow` 设 **位图 + 尺寸 + 屏坐标** |
| 叠层尺寸 | 菜单/设置/提醒 present 使用 **compose 缓冲尺寸**（`hit_size`），**不**依赖 winit 异步 resize 完成后再画 |
| **关坞不缩 HWND** | `dock_hwnd` 记住坞物理框 + 宠槽。`WS_EX_LAYERED`+ULW 窗 **`SetWindowPos` 改尺寸会丢掉位图**，DWM 偶发合成空帧 = 宠闪。关坞只换位图；再开尺寸相同则不再 `SetWindowPos`。 |
| 禁止 winit 改坞窗 | winit `request_inner_size` / `set_outer_position` 用 `SWP_ASYNCWINDOWPOS` + `InvalidateRgn`，尺寸在 ULW **之后**落地，必闪。长大用 `platform::sync_layered_hwnd`（同步、无 `ASYNCWINDOWPOS` / `NOCOPYBITS`）。 |
| 开坞首帧 | 先 `compose_menu_card_layer`，再（如需）同步长大，立刻 `redraw()` |
| 待机画在坞框里 | `compose_pet_in_slot`：宠在原槽，周围透明穿透。禁止把精灵 letterbox 进剩余大 HWND（会把猫拉变形）。 |
| DWM 过渡 | `enable_transparent_window` 设 `DWMWA_TRANSITIONS_FORCEDISABLED`（提醒/哈欠等仍会改尺寸的路径） |
| 设置转场 | 点击肉垫印 → `begin_settings_from_launcher`：`settings_embed`，窗几何锁在启动坞；卡内 220ms `ease_in_out_cubic` 横滑（坞左出、设置从右进）；宠全不透明。落定后 `compose_settings_card` + `hit_settings_card`。设置卡无顶高光（避免圆角双线）。「完成」反转滑回，宠停在提交后的槽位 |
| 开合帧路径 | `about_to_wait` 中 menu 动画进行时 **直接 `redraw()`**，减少 `RedrawRequested` 一跳延迟 |
| 帧率 | `menu_ui_active` **或** `settings_transition.is_some()` → `frame_interval` **16ms（~60fps)**；其它密集态约 33ms |
| 转场命中 | 生长期间 **不**接 settings hit；t=1 后 `finish_settings_transition` 再开 |
| 宠 clip | `MenuOpen` 尽量保持当前 `idle_*` clip，避免强制切 base 引发 crossfade 闪 |

**根因备忘（已修）**

1. 整缓冲全局 alpha 含宠 → 宠先消失。现仅卡/控件 per-layer alpha。  
2. 先 resize 再延迟 paint → 空帧。现原子 present + 立即 redraw。  
2b. **关坞缩回 128、开坞再放大** → 每次开坞都丢 layered 位图。现 `dock_hwnd` 保持坞尺寸。  
2c. 只 ULW、不先把 HWND 撑到卡大小 → 128 窗裁到 union 左上角空白，猫消失、卡不出。长大必须先 `SetWindowPos`（仅首次 / 翻面尺寸变了）。  
3. 30fps + `ease_out_back` → 顿 + 弹。现 60fps + out_quint/out_cubic。  
4. Primary 顶 `INNER_HL` + 底阴影 → 「两道影 / 白条」。现 **flat solid** + 细描边。设置卡同样去掉顶高光 / 内缩圆角高光，否则左上/右上重影。  
5. fontdue 拉丁无 hinting → 波浪字。现 **GDI**（精致字体后期再优化）。  
6. 卡高不够 / `break` 裁行 + `take(5)` → 只显示 2 个。现视口 4 行 + **滚轮** + 软上限 128。  
7. 设置内为宠预留最大体型而改 `card_x` / 叠层尺寸、却不跟 HWND → 设置卡只显示一半。现窗与卡钉死，只动宠槽。

### 5.5 不做

- 系统 Acrylic / 实时抓屏 blur（半透明色 + 高光拟态即可）。  
- 双进程独立启动器窗口。  
- 引入 React / WebView / 真实 Appica npm 包（仅视觉参考）。  
- 现阶段不做更精致字体子系统（用户拍板后期再做）。

---

## 6. 健康提醒

### 6.1 调度

- 默认间隔 60 分钟（设置 15–180）。  
- 单调时钟；暂停整周期不累计误触。  
- 启动补发：**最多一次**，不连弹。  
- `last_completed_at` 写配置。

### 6.2 流程

```text
Due → 存原位（躲边先 snap restore）
    → 一次 sync_layered_hwnd 长大到 union(原位宠, 中央 640×360 卡, 弧线包围盒)
    → 叠层内 sit+squash 轻跃到卡中心宠槽（近距跳过；禁止每帧 set_outer_position）
    → Showing：同缓冲 fade 出本轮卡片（举杯 / 小喇叭轮换）+ 96×96 碗；16ms 直呈
    → Feeding（~900ms）→ 卡 fade 出 → 叠层内轻跃回原位槽
    → begin_idle_present_at(原位) 一次缩回 → Idle
```

拖动中 due → pending；松手后再进。隐藏宠物时 due 可 pending，显示后再出。

**卡片管线**（`render/reminder_ui.rs::load_reminder_card` / `load_reminder_card_deck`）

1. 启动时加载 `ui/reminder_card.png` + `ui/reminder_card_activity.png`（缺一张用剩下的；都缺走程序合成 fallback）。进场 `pick_card_index` 抽一张，避免连抽。  
2. 边界 flood 抠白：相邻近白像素（`BG_MIN_RGB=244`，含最浅抗锯齿环）置透明。
3. 自动裁剪到**内容外框**（+12px 边距），去掉 mockup 四周空白。
4. **premultiplied 缩放**：颜色先乘 alpha 再缩小、最后除回 → 边缘色只来自画面，无白边/暗晕。
5. contain-fit 铺满 640×360，垂直居中；不再预留底部碗带。  
6. 投喂碗 `food_button_layout` 锚在左下（气泡下方透明空档）；热区与碗对齐（`client_to_layout`）。缺图时 fallback 同窗同碗坐标。

---

## 7. 快捷方式与配置

### 7.1 ShortcutItem

字段：`id (Uuid)` · `name` · `target_path` · `arguments` · `working_directory` · `icon_path` · `sort_order` · `enabled` · `launch_count` · `last_launched_at_ms`。

- 启动前校验路径；失效保留 + UI 提示。  
- 删除只删配置条目。  
- 启动：安全 API，禁止危险 shell 拼接。  
- 坞内启动成功：`record_launch` 累加 `launch_count` 并写 `last_launched_at_ms`（serde default，无需 bump schema）。  
- 「最近启用」：`rank_frequent`（enabled + count>0 + 路径仍在；次数降序、并列取新）。

### 7.2 配置

路径：`%APPDATA%/PawDesk/config.json`（以 env 为准）。

- `schema_version` 当前 **3** + `migrate_config`。  
  - v2→v3：将 `pet.scale` 置为产品默认 **0.6**（修正历史写死 1.0 过大）。  
- 主要字段：`pet.scale` / `pet.opacity` / `pet.edge_hide_enabled` · `reminder.*` · `shortcuts[]`（含 `launch_count` / `last_launched_at_ms`） · `window.x/y`。  
- 加载：去 UTF-8 BOM；主配置失败则读 `.bak` 并**回写主文件**；启动 load/migrate 后可立刻 save 固化迁移。  
- 原子写 + `.bak`。  
- 防抖：拖动位置、排序、scale 等。

### 7.3 托盘命令

`TrayCommand`：`ShowPet` · `HidePet` · `PetScaleUp` · `PetScaleDown` · `Exit`。

### 7.4 添加应用 / 文件选择（`shortcut/picker.rs`）

目标：不卡主循环、不闪 launcher、桌面快捷方式显示完整。

| 项 | 约定 |
| --- | --- |
| 线程 | UI 线程只采集 `PickContext`（owner HWND）；**COM STA 工作线程** 弹框 |
| Windows 实现 | 原生 `IFileOpenDialog`，**不用 rfd** 打开对话框 |
| Owner | `set_parent` / `Show(hwndOwner)` 绑定宠物 HWND，**不**切换 `AlwaysOnTop` ↔ `Normal` |
| 起始目录 | `SHGetKnownFolderItem(FOLDERID_Desktop)` → **Shell 虚拟桌面**（用户桌面 ∪ 公共桌面） |
| 禁止 | `SetFolder("%USERPROFILE%\\Desktop")` 纯文件系统路径（会丢 `C:\\Users\\Public\\Desktop`） |
| 禁止 | `FOS_FORCEFILESYSTEM`（强制 FS 视图，破坏虚拟桌面合并） |
| 选项 | `FOS_FILEMUSTEXIST` · `FOS_PATHMUSTEXIST` · `FOS_NODEREFERENCELINKS`（返回 `.lnk` 本身） |
| 过滤器 | 默认「程序 / 快捷方式」`*.lnk;*.url;*.exe`；备选「所有文件」`*.*` |
| 取消 | `HRESULT 0x800704C7` → `None`，不报错 |
| 回主线程 | `UserEvent::FilePicked(Option<PathBuf>)` → 写仓库 / 刷新 UI |
| 非 Windows | 仍走 `rfd` fallback |

入口：`App::begin_pick_executable` → `build_pick_context` → 工作线程 `pick_executable`。

### 7.5 日志

`%LOCALAPPDATA%/PawDesk/logs/` · `tracing`。  
启动应打：`pet_scale`、`pet_logical`、建窗 `logical`/`phys_w`（便于核对缩放是否生效）。

---

## 8. 性能、错误与发布

### 8.1 性能策略

| 状态 | 建议 |
| --- | --- |
| 待机（密帧 blink） | 约 **30 FPS** 呈现亚帧混合；隐藏可再降 |
| **启动坞打开中 / 设置转场** | **~60 FPS**（16ms），保证 scale/fade 丝滑 |
| 设置 / 提醒 / 拖动 | 临时约 30 FPS |
| 目标 | 待机 CPU &lt; 3%、内存 &lt; 200MB（PRD） |

### 8.2 错误

- 统一 `AppError`；用户可读 + 日志细节。  
- 启动失败、选文件取消、配置损坏：不崩，可回退默认/备份。

### 8.3 构建产物与发布

三个目录**不是**三套代码，而是同一源码的不同产物：

| 路径 | 来源 | 用途 |
| --- | --- | --- |
| `target/debug/pawdesk.exe` | `cargo build` / `cargo run` | 开发调试 |
| `target/release/pawdesk.exe` | `cargo build --release` | 日常使用 / 验收最新修复 |
| `dist/PawDesk/` | `tools/package.ps1` 从 **release** 复制（**不入库**，按需生成） | 便携分发包（exe + 精简 assets） |
| `dist/PawDesk-Setup-<ver>.exe` | `tools/make-installer.ps1` + Inno Setup | 给他人安装的演示包（免管理员） |
| `dist/PawDesk-<ver>-portable.zip` | `tools/make-installer.ps1` | 便携 zip |

```text
源码
 ├─ cargo build                 → target/debug/
 ├─ cargo build --release       → target/release/
 ├─ tools/package.ps1           → dist/PawDesk/
 └─ tools/make-installer.ps1    → dist/PawDesk-Setup-<ver>.exe
                                  + dist/PawDesk-<ver>-portable.zip
```

- 改代码后若只跑 `cargo build --release`，**不会**自动更新 `dist/`；要便携包须再跑 `package.ps1`，要安装包再跑 `make-installer.ps1`。  
- 无管理员常规运行；运行时资源优先 exe 旁 `assets/`，开发时回退工程根 `assets/`。  
- 安装包：Inno Setup，per-user 默认 `%LOCALAPPDATA%\Programs\PawDesk`；配置仍在 `%APPDATA%\PawDesk`。

### 8.4 测试

- 单元：状态机、调度、配置、`clamp_pet_scale` / `step_pet_scale`、排序、**place_launcher**、菜单布局。  
- 手工：四边四角开坞、探头开坞、提醒冲突、托盘、DPI 缩放、**宠物变大/变小与设置百分比**、待机眨眼观感。  
- 手工（添加应用）：点「添加应用」launcher **不闪**；对话框桌面可见 **用户 + 公共** 快捷方式；取消后置顶仍正确。

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
| 低层鼠标钩子 | `WH_MOUSE_LL` 只装**专用空闲线程**+自有消息泵；回调仅原子操作；不吞点击；卸载走 `PostThreadMessage(WM_QUIT)` + join |

### 9.2 技术验收（对照 PRD §7）

1. 透明可拖宠物 + 配置恢复。  
2. 多待机 / 30s 撒娇。  
3. 边缘探头。  
4. 鼠标观察（飞扑可选关）。  
5. 提醒投喂闭环。  
6. **钉宠启动坞** 四边四角整卡在 work 内。  
7. 开坞：宠不闪、卡从身边长出、~60fps 丝滑；关坞连续收回。  
8. 快捷方式 CRUD + 启动 + 失效提示。  
9. 托盘完整。  
10. 性能量级达标。  
11. 核心单测通过。

---

## 10. 版本记录

| 版本 | 日期 | 说明 |
| --- | --- | --- |
| v0.1 | 2026-08-01 | 初稿 |
| v0.2 | 2026-08-04 | 钉宠 §7.3、呈现路径 |
| v0.3 | 2026-08-04 | 按模块重排；与 PRD / 钉宠实现同步 |
| v0.4 | 2026-08-06 | 待机眨眼管线 + pet.scale UI |
| v0.5 | 2026-08-07 | 启动坞 Appica + 丝滑动效；60fps；原子 present；防宠闪 |
| v0.6 | 2026-08-07 | **GDI 文字**；列表 **滚轮滚动**（`LIST_VISIBLE_ROWS`/`menu_list_scroll`）；flat primary；对齐 design **v0.8** |
| **v0.7** | **2026-08-10** | 添加应用：原生 `IFileOpenDialog`、Shell 虚拟桌面、不切 z-order；文档澄清 debug/release/dist |
| v0.7.1 | 2026-08-11 | 文档迁入 `docs/`；§2 目录图补 `README.md` / `docs/` / 资源工具 |
| v0.7.2 | 2026-08-11 | `idle_cute` yawn 进调度池 · oneshot settle 书挡 |
| **v0.8** | **2026-08-12** | 设置从启动坞「管理」按钮中心丝滑生长并停靠坞旁（`settings_transition` + `place_settings_near_point` + `settings_rect_at`）；启动坞卡层同步淡出；托盘直开仍居中 |
| **v0.9** | **2026-08-13** | 正面母版 `idle_blink` 3 帧；头眼跟随（姿态条 + 虹膜）；`idle_yawn` + 漫画气泡；旧 cute/stretch 停调度 |
| **v0.10** | **2026-08-13** | 提醒卡片 400×300：flood 抠白 244 + 内容裁边 + premultiplied 缩放（去白边）+ contain-fit 放大；启动坞窗外点击关闭（`OutsideClickGuard`：`WH_MOUSE_LL` 专用钩子线程 + 原子标志，不吞点击）；design **v0.12** · prd **v0.7** |
| **v0.11** | **2026-08-14** | `look_pitch` 从母版品红静帧软抠重画（`pack_pitch_from_gen.py`）；look 身体锁改预乘；哈欠 overlay 复用待机 letterbox，进出体型不跳；卸载旧 cute/stretch/sleep/wag/watch；design **v0.13** · prd **v0.7.1** |
| **v0.12** | **2026-08-14** | 母版 `idle_stretch`（110f@50，键 hold，不拓窗）进 `IDLE_ACTION_ENABLED` 与哈欠轮播；`pack_idle_stretch.py`；design **v0.14** · prd **v0.7.2** |
| **v0.13** | **2026-08-14** | 伸懒腰 7 档（转/下蹲/探/半峰/峰）；峰值设定图表情；1024 抠边+去晕+轮廓 despill；design **v0.15** · prd **v0.7.3** |
| **v0.14** | **2026-08-14** | 提醒进场/回程 `reminder_hop`：攒劲零位移 + ease-in-out 抛物线；`pack_reminder_hop.py`；旅途不再播旧 `reminder_wave`；design **v0.16** · prd **v0.7.4** |
| **v0.15** | **2026-08-17** | 启动坞「最近启用」：`launch_count` / `last_launched_at_ms` + `rank_frequent` + `MenuEntry::Recent`；卡高 **360×450**；design **v0.18** · prd **v0.7.5** |
| **v0.16** | **2026-08-17** | 坞内长按拖动：`list_drag` + 空碗删除；设置去掉常用应用（420×320）；失效行文件框修复；design **v0.19** · prd **v0.7.6** |
| **v0.17** | **2026-08-17** | 坞卡去掉外投影；宠自由剪影且不随卡 scale；拖动分层：长按预热 `ghost/bowl/hint` + 空白卡 + 行位图，插缝只 blit；design **v0.20** · prd **v0.7.7** |
| **v0.18** | **2026-08-17** | 取消托盘暂停/打开设置；设置暂停 `SAY_NO_PAUSE` + `sly_pause`；`pet_scale_draft` 预览 + `idle_present_pos`；拎起行无投影；design **v0.21** · prd **v0.7.8** |
| **v0.19** | **2026-08-18** | `PET_SCALE_MAX=1.0`（设置/托盘同一 `step_pet_scale`）；设置改大小不扩叠层（`sync_pet_slot_to_scale` + 裁剪出界宠）；完成钉 `overlay_origin`；设置卡去顶高光；design **v0.22** · prd **v0.7.9** |
| **v0.20** | **2026-08-18** | 开坞闪：`dock_hwnd` 关坞不缩窗；`sync_layered_hwnd` 替代 winit 异步 resize；`compose_pet_in_slot` 待机画在坞槽；`pet_desk_origin` 持久化宠点；`DWMWA_TRANSITIONS_FORCEDISABLED`；design **v0.23** |
| **v0.21** | **2026-08-18** | 提醒窗 640×360 16:9：插画铺满、碗 96px 落左下；`layout_food_button` 复用 `food_button_layout`；design **v0.24** |
| **v0.22** | **2026-08-18** | 设置改大小后关坞缩回旧尺寸：完成 / `finish_exit_menu_ui` 把 `menu_layout` 宠槽抄回 `dock_hwnd`；design **v0.24** |
| **v0.23** | **2026-08-18** | 开坞/进设置 `overlay_pad_for_max_pet` 预留 100% 宠；`±` 不再裁尾巴；完成兜底 `grow_overlay_to_contain_pet_slot`；dock HWND 只长大不缩小；design **v0.25** · prd **v0.7.10** |
| **v0.24** | **2026-08-18** | 母版 `idle_groom`（110f@50，抬爪/舔/擦颊/擦耳，不拓窗）进 `IDLE_ACTION_ENABLED`；`pack_idle_groom.py`；design **v0.26** · prd **v0.7.11** |
| **v0.25** | **2026-08-18** | 提醒旅途叠层：`place_reminder_travel` + overlay-local hop + 一次 `sync_layered_hwnd` + 60fps 直呈；运行时不再切 `reminder_hop`；design **v0.27** · prd **v0.7.12** |

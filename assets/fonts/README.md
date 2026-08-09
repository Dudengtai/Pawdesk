# 字体包目录（Bundled Fonts）

把字体文件（`.ttf` / `.otf` / `.ttc`）放进本目录，PawDesk 启动后会以进程私有方式
（`AddFontResourceExW` + `FR_PRIVATE`）自动加载，并**优先于系统字体**用于全部界面
文字渲染（菜单、设置面板、提醒气泡）。无需安装到系统、无需管理员权限，卸载程序
时删除文件即可。

## 当前已内置

- **HarmonyOS Sans SC Semibold**（`HarmonyOS_SansSC_Semibold.ttf`，华为官方字形，
  免费商用）——所有界面小字号文字的首选字体，跨机器渲染一致。

## 推荐字体（免费可商用）

| 字体 | 出品方 | 许可证 | 特点 |
|---|---|---|---|
| **MiSans** | 小米 | 免费商用（MIUI 开源） | 字形现代清晰，中文 UI 显示口碑最好，自带多个字重 |
| **HarmonyOS Sans SC** | 华为 | 免费商用 | 为屏幕阅读优化，横竖笔画对比均衡 |
| **思源黑体 Source Han Sans SC**（即 Noto Sans SC） | Adobe / Google | SIL OFL 1.1 | 开源标准黑体，全球分发一致 |
| **阿里巴巴普惠体 Alibaba PuHuiTi 3.0** | 阿里巴巴 | 免费商用 | 覆盖广，字体包体积小 |

建议放置 **Medium / Semibold（600）字重** 的单个文件即可（如 `MiSans-Semibold.ttf`），
界面小字号会直接受益。多放几个字重更好，但每个约 5–10 MB，会增大安装包体积。

> 说明：本目录为空时，界面自动回退到系统字体栈：
> Microsoft YaHei UI → Microsoft YaHei → Segoe UI → DengXian → SimHei，
> 小字号（≤28 设备像素）自动使用 Semibold 字重保证清晰度。

## 支持的面族名（按优先级）

```
HarmonyOS Sans SC
MiSans
Noto Sans SC
Source Han Sans SC
Alibaba PuHuiTi 3.0
Alibaba PuHuiTi
（以下为系统回退）
Microsoft YaHei UI
Microsoft YaHei
Segoe UI
DengXian
SimHei
```

字体文件名不限，只要文件内的**面族名（family name）**命中上表即可。其他面族名的
字体放进来会被注册，但不会成为默认面（可自行在 `src/render/text.rs` 的
`FACE_PRIORITY` 中追加）。

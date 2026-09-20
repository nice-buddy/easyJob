# Changelog

> 发布流程会在每次 push tag 时读取本文件内容，作为 GitHub Release 的说明。
> 发布前请先更新本文件，写清该版本的变更。章节标题格式固定为 `## v<版本号>`，
> 必须与 tag 名称（如 `v1.0.1`）去掉前缀 `v` 后对应，以便 workflow 自动提取。

## v1.0.2

### 修复
- 修复 Windows 网络触发器事件方向颠倒的问题：禁用外网网卡曾误报「连接网络」，启用曾误报「断开网络」
- Windows 网络事件改为按「整机能否出外网」判定，并统一处理有线网卡与 WiFi 通知；其它网卡或 WiFi 状态抖动不再凭空产生事件
- 修复联网探测把「域名能解析」误判为「能上网」的问题，并消化启用网卡时 DHCP / 路由尚未就绪的过渡期
- 修复 Windows 下 cmd / PowerShell 执行日志中文乱码

### 变更
- cmd / PowerShell 输出编码默认改为 GBK（代码页 936），可在动作配置中切换为 UTF-8
- 项目版本号统一为 1.0.2（workspace 各 crate、桌面端 package.json、tauri.conf.json）
- 新增 `.gitattributes` 与 `.editorconfig`，统一 macOS 与 Windows 的换行符、编码与文件权限位，避免纯换行符差异被识别为本地文件变更
- `.gitignore` 补充 Windows 与 IDE 噪音文件（`Thumbs.db`、`desktop.ini`、`.idea/`、`*.iml`）

## v1.0.1

### 新增
- 系统设置新增「版本与更新」卡片：展示当前版本，进入页面自动检查一次，也支持手动检查
- 检查更新基于 GitHub Release 记录：有新版本时展示发布时间、更新内容摘要，并提供「前往下载新版本」跳转
- 仓库暂无已发布版本时显示「暂无发布」空态，不再报错

### 变更
- 项目版本号统一为 1.0.1（ workspace 各 crate、桌面端 package.json、tauri.conf.json）
- 设置页移除 Agent 版本展示，仅保留 App 版本
- 发布流程改为 push tag 后自动发布正式 Release（不再是草稿），Release 说明从本文件对应章节自动提取

### 修复
- 修复仓库无 Release 时检查更新报 404 的问题（改用 releases 列表接口 + 空态处理）

# ModelMonitor

基于 [CC Switch](https://github.com/farion1231/cc-switch)（MIT 协议，v3.19.1）魔改的 **AI 编码工具供应商切换 + 模型用量监控** 工具。

> 原功能全部保留（Claude Code / Codex / Gemini CLI / Grok Build / OpenCode 的供应商切换、配置管理、代理路由、用量统计、会话同步等），在此基础上重做了托盘体验，让"当日每个模型花了多少 token、多少钱"一眼可见。

## 魔改内容

### 1. 托盘「今日用量」监控
- 标题行「今日用量」不展开，下方按应用分组的子菜单（带应用图标）平铺，组内一行一个模型：
  ```
  今日用量
  [icon] codex ▸
    gpt-5.6-sol       │ 265.8w 1.54$
    gpt-5.6-terra     │ 53.3w  0.25$
  [icon] opencode ▸
    deepseek-v4-flash │ 2019.4w 0.60¥
  ```
- 模型列按显示宽度（ASCII=1、CJK=2）补齐后接 `│` 竖线分隔，数字列视觉对齐
- 按应用成本降序排列，应用内模型按成本降序；最多 6 个应用、每应用 10 行
- token 以 `w`（万）/`亿` 显示；成本按供应商/模型区分 `$` / `¥`（国内模型按 1 USD ≈ 7.14 折算）
- 每 60 秒随会话同步自动刷新

### 2. 默认轻量模式
- 启动即进入轻量模式：主窗口销毁，只保留托盘图标（不占 Dock）
- 点「打开主界面」临时重建窗口

### 3. 托盘精简
- 移除「轻量模式」「打开官方网站」两个菜单项

### 4. 托盘加入 OpenCode 分区
- 上游托盘仅支持 Claude / Codex / Gemini / Grok Build，本分支把 OpenCode 加入 `TRAY_SECTIONS`，可直接在托盘切换 OpenCode 供应商

## 数据来源

用量数据来自 CC Switch 自身的记账体系：

- `proxy_request_logs` 表：本地代理路由的实时请求 + Codex / OpenCode / Gemini 会话日志的分钟级同步（启动时全量回填费用，之后每 60s 增量导入）
- 按**本地自然日**聚合（`date(datetime(created_at,'unixepoch','localtime')) = date('now','localtime')`）
- token 口径：input + output + cache_read + cache_creation

## 关键文件

| 文件 | 说明 |
|---|---|
| `src-tauri/src/services/model_monitor.rs` | 今日用量查询（`get_today_usage_by_app_grouped`）、token/成本格式化、单元测试 |
| `src-tauri/src/tray.rs` | 托盘菜单：今日用量区（应用图标子菜单 + `│` 竖线对齐列）、OpenCode 分区、菜单精简 |
| `src-tauri/src/lib.rs` | 启动流程：默认进入轻量模式；会话同步后刷新托盘 |
| `src-tauri/icons/apps/` | 应用图标 PNG（由 `src/icons/extracted/*.svg` 经 `sips` 转换，预留给后续托盘图标徽标） |

## 构建

```bash
# 环境：Rust stable + Node.js 20+ + pnpm 10.x
cd src-tauri
cargo build --release            # 或直接:
cd .. && pnpm install
# 对齐 tauri 前后端版本（仓库 Cargo.lock 与 npm 包版本需一致）：
pnpm add -E @tauri-apps/api@2.10.1 @tauri-apps/plugin-dialog@2.6.0 @tauri-apps/plugin-updater@2.10.0
./node_modules/.bin/tauri build
# 产物：
#   src-tauri/target/release/bundle/macos/CC Switch.app
#   src-tauri/target/release/bundle/dmg/CC Switch_3.19.1_aarch64.dmg
```

安装：备份原 `/Applications/CC Switch.app` 后整体替换（Bundle ID 相同，数据目录 `~/.cc-switch` 无缝沿用）。

## 常见问题

- **托盘看不到用量**：确认 `proxy_request_logs` 有当日数据（主窗口用量面板能看到即有）；SQL 中 `GROUP BY` 必须用位置序号（`GROUP BY 1,2`），因为 `providers` 表也有 `app_type` 列，按列名分组会报 `ambiguous column name`。
- **构建报版本不匹配**：见上方 `pnpm add -E` 步骤，以 Cargo.lock 中的 tauri 版本为准对齐。

## 许可

上游 CC Switch 为 MIT；本分支修改同样以 MIT 发布。

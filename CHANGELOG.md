# Changelog

本文档记录 NSH DT 的版本变更。版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/) 约定。

## [Unreleased]

### Added

- 答题历史记录功能：在「设置 → 答题记录」开关启用后，每答完一题在 `data/history.jsonl` 追加一行 JSON，包含题目文本、诗词库命中、AI 1/2 答案和模型、各阶段耗时。默认关闭，仅本地落盘，不传任何服务器。
- 开源准备：补齐 LICENSE、CONTRIBUTING、CHANGELOG、Issue/PR 模板与 GitHub Actions CI。

## [0.2.0]

基于 Tauri 2 + React 18 + Rust 的全新重构版本。

### Added

- Tauri 2 桌面壳，替换早期 Electron 实现。
- Rust workspace：`core` / `ai` / `ocr` / `poetry` / `api` 五个 crate。
- 截图区域选择窗口，支持快速截取题目区域。
- 百度 OCR 集成，截图转文本。
- 本地 SQLite 诗词库 + 子句索引，毫秒级匹配 5 字 / 7 字诗句。
- 双 AI 通道（AI 1 / AI 2）并行回答，互不阻塞。
- 多服务商预设：OpenAI、Gemini、DeepSeek、火山方舟、SiliconFlow、Custom。
- 10 秒超时控制，避免长时间卡住。
- 关闭主窗口时清理截图窗口与子进程。

## 历史版本

`portable-v4.x` 与 `portable-react-v5` 为重构前的 Electron + Vue + Flask/Python 版本，已不再维护，仅作为历史归档保留在本机。

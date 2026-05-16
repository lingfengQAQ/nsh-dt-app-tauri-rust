# 逆水寒殿试答题器全量重构计划

## 目标

将现有 Electron + Vue + Flask/Python 架构重构为 Tauri + Vue + Rust 后端的一体化桌面应用，优先解决当前版本的 AI 配置不可用、打包复杂、启动慢、后端路径不稳定、答题链路等待时间长等问题。

## 总体原则

- 保留现有 Vue 业务界面和交互习惯，减少前端重写成本。
- 后端能力迁移到 Rust，避免 Python/uv/PyInstaller 运行链路。
- 优先保持现有 API/数据格式兼容，便于逐步迁移和对比验证。
- 答题链路按“谁先有答案谁先显示”的原则重构，避免 AI 网络耗时拖慢本地命中结果。
- 打包产物必须单目录可运行，配置、数据库、索引路径清晰可控。

## 目标目录结构

```text
nsh-dt-app-tauri-rust/
├── PLAN.md
├── app/                         # Tauri + Vue 前端
│   ├── src/                     # 复用/改造现有 Vue 页面
│   ├── src-tauri/               # Tauri Rust 宿主
│   └── package.json
├── crates/
│   ├── core/                    # 通用类型、错误、配置、路径
│   ├── ai/                      # AI 服务、模型列表、流式回答
│   ├── ocr/                     # OCR 服务：仅百度 OCR
│   ├── poetry/                  # 诗词库、索引、查询、缓存
│   └── api/                     # Axum/Tauri command 适配层
├── data/
│   ├── settings.example.json
│   └── README.md
├── scripts/
│   ├── migrate-settings.ps1
│   ├── build-index.ps1
│   └── package-windows.ps1
└── docs/
    ├── api-compat.md
    ├── packaging.md
    └── performance.md
```

## 阶段 0：基线确认

- 记录当前 Electron/Flask 版可用 API：health、settings、ai、ocr、knowledge_base。
- 固化测试样例：AI 配置保存、模型拉取、OCR 返回、诗词字符题命中、普通搜索。
- 准备一组真实截图和 OCR 文本作为回归样本。
- 记录当前性能基线：启动时间、截图到 OCR 时间、诗词命中时间、AI 首字时间、总答题时间。

## 阶段 1：项目骨架

- 创建 Tauri + Vue 项目骨架。
- 建立 Rust workspace 和基础 crates。
- 增加统一日志、错误类型、配置路径解析。
- 先提供 `/api/health` 或等价 Tauri command，验证前后端通信。
- Windows 下验证开发启动、热更新、打包基础流程。

## 阶段 2：设置系统重写

- 兼容读取旧版 `settings.json`。
- 配置路径统一：开发目录、便携版目录、用户数据目录三种模式明确区分。
- 支持 AI/百度 OCR/界面设置的读写、导入、导出、重置。
- 对敏感字段做最小化日志输出，避免 API Key 出现在日志里。
- 前端设置页先迁移到新设置接口。

## 阶段 3：AI 模块重写

- 支持 OpenAI-compatible Chat Completions。
- 支持自定义 Base URL。
- 预填常用服务商 API URL：
  - OpenAI：`https://api.openai.com/v1`
  - DeepSeek：`https://api.deepseek.com/v1`
  - 火山方舟：`https://ark.cn-beijing.volces.com/api/v3`
  - 硅基流动：`https://api.siliconflow.cn/v1`
- 通过服务商 `/models` API 获取模型 ID 列表。
- 支持手动输入自定义模型 ID。
- 增加 AI 流式回答，前端显示首字时间。
- 增加竞速策略：多个 AI 并发时，第一个成功结果先展示。
- 增加超时、取消、错误分级和重试策略。

## 阶段 4：诗词库模块重写

- 继续使用 SQLite 作为桌面版默认存储，不引入 Redis。
- 重新设计索引表：
  - `poems(id, title, author, dynasty, content_json)`
  - `clauses(id, poem_id, text, normalized_text, len)`
  - `char_index(char, clause_id)`
  - `char_freq(char, freq)`
- 预构建索引，避免首次启动现场构建超大索引。
- 查询流程优化：
  - 输入字符归一化。
  - 使用低频字符快速缩小候选。
  - 候选 clause 做字符计数校验。
  - LRU 缓存 `sorted_chars -> result`。
- 普通搜索增加 SQLite FTS 或轻量关键词索引。
- 前端优先展示本地诗词命中结果，不等待 AI。

## 阶段 5：OCR 模块重写

- 仅保留百度 OCR，移除本地 Tesseract OCR 配置、路径选择和调用逻辑。
- 使用 Rust `reqwest` 实现百度 Access Token 获取、缓存、过期刷新和请求复用。
- 增加图片预处理：裁剪、灰度、压缩、尺寸限制，降低百度 OCR 上传耗时。
- OCR 结果进入统一文本清洗管线。
- 设置页只保留百度 API Key、Secret Key 和 OCR 相关高级参数。

## 阶段 6：截图和窗口能力迁移

- 用 Tauri/Rust 实现区域截图、全屏截图、截图框窗口。
- 支持记住上次截图区域。
- 支持全局快捷键：截图、识别、显示/隐藏主窗口。
- 修复多屏和缩放问题，确保副屏、负坐标、高 DPI 可用。

## 阶段 7：答题链路重构

- 新链路：截图 -> OCR -> 本地诗词库 -> AI 流式并行补充。
- 诗词题：本地命中后立即显示答案，AI 可选后台补充。
- 非诗词题：OCR 后立即发 AI 流式请求。
- 前端显示分阶段耗时：截图、OCR、知识库、AI 首字、AI 完成。
- 增加“极速模式”：只用本地库或只取第一个 AI 答案。

## 阶段 8：兼容和迁移

- 提供旧配置迁移脚本。
- 支持从旧版 `resources/settings.json` 导入配置。
- 保持旧版导入/导出 JSON 格式兼容。
- 文档说明旧版到新版的迁移步骤。

## 阶段 9：测试和性能验收

- Rust 单元测试：配置、AI URL、模型列表解析、诗词字符匹配。
- 集成测试：模拟 OpenAI-compatible 服务、百度 OCR mock、SQLite 查询。
- 前端构建测试：Vue/Tauri build。
- 性能验收目标：
  - 冷启动明显低于 Electron + Flask 版。
  - 本地诗词命中在索引就绪后稳定毫秒级到百毫秒级。
  - AI 首字以流式方式尽早展示。
  - 打包后不依赖 Python、uv、PyInstaller。

## 第一批实施任务

1. 初始化 `app/` Tauri + Vue 项目。
2. 初始化 Rust workspace 和 `core`、`ai`、`api` crates。
3. 实现设置读写和路径管理。
4. 实现 AI 模型列表获取接口。
5. 实现 AI 流式回答接口。
6. 将现有设置页 AI 弹窗迁移到新接口。
7. 做一个最小可运行版本，先不迁移诗词库，OCR 只规划百度接口。

## 风险点

- Tauri 截图和透明窗口实现可能需要 Windows 平台专项处理。
- 诗词库索引体积需要重新评估，避免再次生成 1GB 级冗余索引。
- 不同服务商 `/models` 返回格式不完全一致，需要宽松解析。
- 火山方舟部分模型可能要求 endpoint/model ID 由控制台创建，模型列表接口不一定总能返回用户可用模型。

## 暂不做

- 不引入 Redis 作为桌面版默认依赖。
- 不一次性重写全部 UI。
- 不把 AI Key 写入日志。
- 不直接删除旧项目，直到新版功能对齐。




## 当前进度（2026-04-25）

- 已创建 `D:\wk\nsh-dt-app-tauri-rust` 新项目，并复制旧版 `resources\poetry.db` 到 `data\poetry.db`。
- 已搭建 Tauri + Vue + Rust workspace：`core`、`ai`、`ocr`、`poetry`、`api`、`app/src-tauri`。
- 已实现 AI 服务商预设、自定义 Base URL、OpenAI-compatible `/models` 拉取模型列表，前端仍允许手动输入模型名。
- 已实现配置读写命令，AI Key、百度 OCR Key 返回前会脱敏，前端可保存 AI 和百度 OCR 配置。
- 已实现百度 OCR-only 命令 `baidu_ocr_base64`，不再接入本地 OCR/Tesseract。
- 已实现诗词库基础查询命令 `search_poetry`、`match_poetry_from_text`，当前先直接兼容旧 `poetry.db`。
- 已在前端加入 AI 配置、百度 OCR、诗词库查询三个可操作区块，形成“图片/OCR 文本 -> 诗词查询”的最小闭环。
- 已通过 `cargo check --workspace`、`cargo test --workspace`、`npm run build`。

## 下一批实施任务

1. 为 `poetry.db` 生成轻量 `poetry_index.db` 或内置 FTS/倒排索引，替代当前大表扫描。
2. 增加 AI Chat Completions 流式回答命令，并在诗词未命中时自动进入 AI 回答。
3. 增加分阶段耗时统计：OCR、诗词库、AI 首字、AI 完成。
4. 接入 Tauri 截图/区域截图/快捷键，把手动图片输入替换为真实截图流程。
5. 补旧配置迁移：从旧版 `win-unpacked\resources` 导入 AI/OCR 设置。

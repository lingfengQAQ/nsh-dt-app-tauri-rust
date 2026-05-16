# API 兼容策略

新版优先提供 Tauri commands；为了降低迁移风险，命名和返回结构尽量贴近旧 Flask API。

## 第一阶段命令

- `health`：返回 `{ success, status, version }`
- `get_settings` / `save_settings`
- `list_ai_models`
- `ask_ai_stream`
- `recognize_baidu_ocr`
- `find_poem_from_chars`

## 答题事件流

长期目标是统一成一个答题事件流：

1. `ocr_started`
2. `ocr_text`
3. `poetry_hit` 或 `poetry_miss`
4. `ai_delta`
5. `ai_done`
6. `timing`

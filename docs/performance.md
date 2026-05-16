# 性能目标

## 用户体感目标

- 本地诗词命中不等待 AI。
- AI 使用流式展示，记录首字时间。
- 百度 OCR 上传前压缩图片，减少网络耗时。

## 技术目标

- 不依赖 Python/uv/PyInstaller。
- 不复制旧版 1GB `poetry_index.db`。
- 优先 SQLite + LRU，不引入 Redis。

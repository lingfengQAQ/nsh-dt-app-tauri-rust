# 数据目录

- `poetry.db`：从旧版打包目录 `C:\Users\lcy\Desktop\win-unpacked\resources\poetry.db` 复制过来的原始诗词库。
- `poetry_index.db`：旧版索引文件暂不复制；新版 Rust 后端会重新设计索引表并重新构建，避免沿用旧版约 1GB 的冗余索引。
- `poetry_clause_index.db`：新版子句索引，可用 `scripts/build-clause-index.py` 从本地 `poetry.db` 生成。
- `settings.example.json`：后续初始化 Rust/Tauri 配置系统时补充。

数据库文件体积较大，默认不提交到 git；本地开发和打包时保留在 `data/` 目录即可。

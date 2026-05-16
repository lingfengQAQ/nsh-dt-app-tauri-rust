# 数据目录

本目录存放运行期数据，主要是诗词数据库文件。所有 `*.db` 默认被 `.gitignore` 忽略，不进仓库。

## 文件说明

- `poetry.db`：诗词原始库（SQLite）。
- `poetry_clause_index.db`：子句反向索引，用于加速字符匹配查询，可由 `scripts/build-clause-index.py` 从 `poetry.db` 生成。
- `settings.example.json`：配置模板。首次运行可拷贝为 `settings.json` 并填入自己的 API Key。

## 准备数据库

`poetry.db` 不在仓库中，请自行准备：

- 自行收集诗词数据并构建一个包含 `id, title, author, dynasty, content` 等列的 SQLite 表。
- 或者从其他公开数据源导入后放到 `data/poetry.db`。

放好 `poetry.db` 后，生成子句索引：

```powershell
python scripts/build-clause-index.py data/poetry.db data/poetry_clause_index.db
```

也可以通过环境变量指定数据库位置：

```powershell
$env:NSH_POETRY_DB = "D:\path\to\poetry.db"
```

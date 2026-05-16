# NSH DT

NSH DT 是一个基于 Tauri、React 和 Rust 的桌面答题辅助工具。它面向 Windows 桌面使用场景，支持截图取题、百度 OCR、本地诗词知识库检索，以及双 AI 通道并行回答。

> ⚠️ **免责声明**
>
> 本项目仅供个人学习、技术研究与离线诗词检索等合法用途。请使用者自行评估并承担在任何特定第三方软件、平台或服务中使用本工具可能引发的全部风险（包括但不限于账号封禁、违反对方服务条款等）。作者不对使用者因使用本项目而产生的任何直接或间接后果负责，禁止将本项目用于任何商业用途或可能损害他人权益的场景。下载、复制或运行本项目即视为已阅读并同意本声明。

## 功能特性

- 截图区域选择：打开可拖拽的截图选择窗口，快速截取题目区域。
- OCR 识别：通过百度 OCR 将截图内容转换为文本。
- 本地诗词库：优先使用 SQLite 诗词库和子句索引匹配 5 字、7 字诗句。
- 双 AI 通道：AI 1 和 AI 2 可同时回答，互不阻塞。
- 灵活模型配置：每个 AI 通道可单独配置 API URL、API Key 和模型，也可以让 AI 2 复用 AI 1 的接口地址和密钥。
- 超时控制：本地知识库和 AI 回答均有 10 秒上限，避免长时间卡住。
- 退出清理：关闭主窗口时会销毁截图窗口并退出进程，避免后台残留。

## 技术栈

- 桌面框架：Tauri 2
- 前端：React 18 + Vite
- 后端：Rust 2021
- 数据库：SQLite
- HTTP：reqwest
- OCR：百度 OCR
- AI 接口：OpenAI 兼容 Chat Completions API

## 目录结构

```text
.
├── app/                 # Tauri 桌面应用和 React 前端
│   ├── src/             # 前端源码
│   └── src-tauri/       # Tauri Rust 宿主
├── crates/
│   ├── ai/              # AI 客户端
│   ├── api/             # API 类型与兼容层
│   ├── core/            # 配置、路径和通用逻辑
│   ├── ocr/             # 百度 OCR 客户端
│   └── poetry/          # 诗词库查询与索引逻辑
├── data/                # 本地数据文件，数据库默认不提交
├── docs/                # 设计和兼容说明
└── scripts/             # 检查、迁移和索引构建脚本
```

## 快速开始

普通用户直接到 [Releases](../../releases/latest) 下载最新的 `nsh-dt-x.x.x-x64-setup.exe` 双击安装即可。诗词数据库已经内置在安装包中，**无需额外下载任何数据文件**。

首次运行后，在设置里填入你自己的百度 OCR 和 AI 服务商 Key。

## 本地数据（仅开发者关心）

> 普通用户跳过本节即可。

如果你想从源码构建，需要准备两个 SQLite 文件放到 `data/` 目录：

- `data/poetry.db`：诗词原始库
- `data/poetry_clause_index.db`：子句反向索引

这两个文件体积较大，默认被 `.gitignore` 忽略，不会进仓库。Tauri 打包时会把它们作为资源带进安装包，所以装完的 `.exe` 用户开箱即用。

如果你只有 `poetry.db`，可以用脚本生成 `poetry_clause_index.db`：

```powershell
python scripts/build-clause-index.py data/poetry.db data/poetry_clause_index.db
```

也可以通过环境变量指定诗词库位置：

```powershell
$env:NSH_POETRY_DB = "D:\path\to\poetry.db"
```

## 开发

安装前端依赖：

```powershell
cd app
npm install
```

启动 Tauri 开发模式：

```powershell
npm run tauri:dev
```

仅构建前端：

```powershell
npm run build
```

运行 Rust 测试：

```powershell
cargo test --workspace
```

## 打包

在 `app/` 目录执行：

```powershell
npm run tauri:build
```

构建完成后，常用产物位于：

```text
target/release/nsh-dt-tauri.exe
target/release/bundle/nsis/
target/release/bundle/msi/
```

## 配置

应用会自动读取或创建本地配置文件。便携目录存在 `data/` 时，会优先使用便携目录下的 `data/config.json`；否则使用系统应用数据目录。

主要配置项包括：

- `ai`：AI 1 配置
- `ai_secondary`：AI 2 配置
- `baidu_ocr`：百度 OCR `app_id`、`api_key`、`secret_key`
- `settings`：语言、主题和自动保存设置

可以参考 `data/settings.example.json`。不要把真实 API Key 提交到 git。

## AI 预设

内置模型供应商预设包括：

- OpenAI
- Google Gemini
- DeepSeek
- Volcengine Ark / Doubao
- SiliconFlow
- Custom

所有请求走 OpenAI 兼容的 Chat Completions 结构。普通题默认要求直接给出简短答案；诗词题默认只返回最可能的 5 字或 7 字诗句。

## 贡献

欢迎通过 Issue 反馈问题或通过 Pull Request 提交改进，详见 [CONTRIBUTING.md](CONTRIBUTING.md)。版本历史见 [CHANGELOG.md](CHANGELOG.md)。

## 开源协议

本项目使用 MIT License，详见 [LICENSE](LICENSE)。

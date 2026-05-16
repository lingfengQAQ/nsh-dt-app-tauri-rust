# 贡献指南

感谢你对 NSH DT 的关注！欢迎以多种方式参与贡献。

## 提交 Issue

提 Issue 之前请先：

1. 在 [Issues](../../issues) 中搜索关键字，避免重复。
2. 升级到最新版本试试，确认问题仍然存在。
3. 使用对应的 Issue 模板（Bug Report / Feature Request）填写信息。

报告 Bug 时，请尽量提供：

- 操作系统版本（Windows 10 / 11，构建号）
- 应用版本号
- 复现步骤、预期行为与实际行为
- 必要的截图或日志（**请隐去 API Key、Token 等敏感信息**）

## 提交 Pull Request

1. Fork 本仓库并基于 `main` 创建特性分支：`git checkout -b feat/xxx`。
2. 改动尽量集中、聚焦，一个 PR 解决一件事。
3. 提交前请本地通过：
   ```powershell
   cargo fmt --all
   cargo test --workspace
   cd app && npm run build
   ```
4. 提交信息请使用清晰的中文或英文短句，例如：`fix: 修复诗词索引在空字符时崩溃`。
5. 在 PR 描述中说明改动动机、影响范围和测试方式。

## 代码风格

- **Rust**：使用 `cargo fmt`，遵循 Rust 2021 编辑约定。
- **前端 (React)**：保持现有 JSX/JS 风格，不引入额外格式化工具时，请遵循当前缩进与命名习惯。
- 提交里**不要包含**：API Key、Token、个人路径、二进制构建产物、`data/*.db`。

## 安全相关

如发现安全相关问题（如可能泄露密钥、远程命令执行等），请勿直接公开 Issue，先通过私信或邮件方式联系维护者。

## 行为准则

请保持友善、互相尊重。技术讨论对事不对人。

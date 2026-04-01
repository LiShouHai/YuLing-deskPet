# Contributing Guide

感谢关注「隅灵」桌宠项目。为保持代码质量与跨平台一致性，请在贡献前阅读以下约定。

## 分支策略
- `main`: 永远保持可发布状态，所有功能通过 PR 合并。
- 功能/修复分支：使用 `feature/<topic>` 或 `fix/<topic>` 命名，例如 `feature/platform-bridge`。
- 提交前确保变更已在 macOS (必要时 Windows) 上验证。

## 提交流程
1. 从最新 `main` 创建分支。
2. 执行必要的构建/测试命令：
   - `pnpm install`
   - `pnpm lint`
   - `cargo fmt && cargo clippy`（位于 `src-tauri`）。
3. 使用语义化的 commit message，例如 `feat: add reminder engine`、`fix: adjust drag easing`、`docs: update plan`。
4. 提交 PR，附上变更说明与验证截图/日志。

## 代码约定
- 前端：Vue 3 + TypeScript，使用 Composition API、Pinia；样式遵循 Tailwind Utility-first。
- 动画资源：放入 `assets/motion/`，遵循 manifest 约定。
- 后端：Rust stable，保持 `cargo fmt` 与 `cargo clippy -- -D warnings` 通过。
- 文档：设计与计划更新存放在 `docs/superpowers/`，完成阶段后及时勾选 Todo 并追记日期。

## Issue 与 PR
- 新功能：创建 Issue 描述动机、范围、验收标准。
- Bug 修复：提供复现步骤、期望/实际行为、环境信息。
- PR 需至少自测 + 1 名审核者通过方可合并。

欢迎提交问题或建议，感谢贡献！

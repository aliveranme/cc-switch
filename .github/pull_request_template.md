## Summary / 概述

<!-- Briefly describe what this PR does and why. / 简要描述这个 PR 做了什么以及为什么。 -->

## Related Issue / 关联 Issue

<!-- Link the related issue. Use "Fixes #123" to auto-close it when merged. -->
<!-- 关联相关 Issue。使用 "Fixes #123" 可在合并时自动关闭。 -->

Fixes #

## Screenshots / 截图

<!-- If applicable, add before/after screenshots. / 如有需要，请添加修改前后的截图。 -->

| Before / 修改前 | After / 修改后 |
|-----------------|---------------|
|                 |               |

## Checklist / 检查清单

<!-- These mirror the CI jobs one-to-one; running them locally avoids a red PR. -->
<!-- 这些与 CI 的检查项一一对应，本地先跑可以避免 PR 变红。 -->

- [ ] `pnpm typecheck` passes / 通过 TypeScript 类型检查
- [ ] `pnpm format:check` passes / 通过代码格式检查
- [ ] `pnpm test:unit` passes / 通过前端单元测试
- [ ] `pnpm build:renderer` passes / 通过前端构建
- [ ] `cargo fmt --check` passes (if Rust code changed) / 通过 Rust 格式检查（如修改了 Rust 代码）
- [ ] `cargo clippy --all-targets -- -D warnings` passes (if Rust code changed) / 通过 Clippy 检查（如修改了 Rust 代码）
- [ ] `cargo test` passes (if Rust code changed) / 通过 Rust 测试（如修改了 Rust 代码）
- [ ] Updated all four locales if user-facing text changed / 如修改了用户可见文本，已同步更新四个语言文件（zh / zh-TW / en / ja）

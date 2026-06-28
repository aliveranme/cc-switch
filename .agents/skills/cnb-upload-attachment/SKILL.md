---
name: cnb-upload-attachment
description: 上传附件（图片、文档、压缩包等）到 Issue 或 PR 评论，返回 Markdown 附件链接。
---

# 上传附件到 Issue / PR

将本地文件上传到 CNB 平台的 Issue 或 PR，返回可嵌入评论的 Markdown 附件链接。

## 使用场景

- 在 Issue 或 PR 评论中附加截图、日志文件、构建产物等
- 上传图片（png/jpg/gif/svg/webp）或普通文件（zip/pdf/log 等）

## 执行步骤

### 1. 判断文件类别

根据文件扩展名判断使用哪个命令：

- **图片**（`.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, `.webp`, `.ico` 等图片类型）→ 使用 `upload-image`
- **其他文件** → 使用 `upload-file`

### 2. 执行上传

上传图片：

```bash
cnb issues upload-image --file 本地文件路径
cnb pulls upload-image --file 本地文件路径
```

上传普通文件：

```bash
cnb issues upload-file --file 本地文件路径
cnb pulls upload-file --file 本地文件路径
```

### 3. 使用上传结果

命令返回 JSON，其中包含 `asset_link`（Issue）或 `path`（PR）字段，即附件的访问链接。

附件在评论中的 Markdown 格式：

- 图片：`![文件名](附件URL)` — 渲染为内联图片
- 普通文件：`[文件名](附件URL)` — 渲染为下载链接
import React, { lazy, Suspense } from "react";
import type { JsonEditorProps } from "./JsonEditorImpl";

/**
 * CodeMirror 是打包体积最大的单个依赖（约 640 kB）。它只服务于编辑器，
 * 而编辑器全部位于模态框或表单区块内，用户不打开就用不到，因此实现下沉到
 * JsonEditorImpl 并按需加载。
 *
 * 包装保持 default export 与原来一致，所有调用点无需改动。
 */
const JsonEditorImpl = lazy(() => import("./JsonEditorImpl"));

const JsonEditor: React.FC<JsonEditorProps> = (props) => {
  const { height, rows = 12 } = props;
  const isFullHeight = height === "100%";

  // 与 JsonEditorImpl 的高度计算保持一致，避免 chunk 到达时布局跳动
  const resolvedHeight =
    typeof height === "number" ? `${height}px` : (height ?? undefined);

  return (
    <Suspense
      fallback={
        <div
          className="w-full animate-pulse rounded-md bg-muted/40"
          style={{
            height: isFullHeight ? "100%" : resolvedHeight,
            minHeight: height ? undefined : `${Math.max(1, rows) * 18}px`,
          }}
        />
      }
    >
      <JsonEditorImpl {...props} />
    </Suspense>
  );
};

export default JsonEditor;
import React, { lazy, Suspense } from "react";
import type { MarkdownEditorProps } from "./MarkdownEditorImpl";

/**
 * 与 JsonEditor 同样的理由：CodeMirror 只在用户打开编辑器时才需要，
 * 实现下沉到 MarkdownEditorImpl 并按需加载。两者共用同一个 CodeMirror
 * chunk，先加载的那个会把它带进来。
 */
const MarkdownEditorImpl = lazy(() => import("./MarkdownEditorImpl"));

const MarkdownEditor: React.FC<MarkdownEditorProps> = (props) => {
  const { className = "", minHeight = "300px" } = props;

  return (
    <Suspense
      fallback={
        <div
          className={`border rounded-md overflow-hidden animate-pulse bg-muted/40 ${className}`}
          style={{ minHeight }}
        />
      }
    >
      <MarkdownEditorImpl {...props} />
    </Suspense>
  );
};

export default MarkdownEditor;

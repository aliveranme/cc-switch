import { act, render, waitFor } from "@testing-library/react";
import { EditorView } from "@codemirror/view";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";

import JsonEditor from "@/components/JsonEditor";

// fork 将 JsonEditor 实现下沉到 JsonEditorImpl 并按需 lazy 加载（CodeMirror
// 是体积最大的单个依赖）。渲染后 `.cm-content` 要等 Suspense fallback 结束、
// lazy chunk 到达后才挂载，因此测试必须先等待它出现，再查询编辑器视图。
async function renderEditor(props: ComponentProps<typeof JsonEditor>) {
  const utils = render(<JsonEditor {...props} />);
  const content = await waitFor(() => {
    const el = utils.container.querySelector(".cm-content");
    expect(el).not.toBeNull();
    return el as HTMLElement;
  });
  return { ...utils, content };
}

describe("JsonEditor", () => {
  it("updates height and callbacks without recreating the editor view", async () => {
    const firstOnChange = vi.fn();
    const secondOnChange = vi.fn();
    const { container, rerender, content } = await renderEditor({
      id: "configuration-json",
      ariaLabel: "Configuration JSON",
      value: "{}",
      onChange: firstOnChange,
      height: 60,
    });

    const originalView = EditorView.findFromDOM(content);
    expect(originalView).toBeDefined();
    expect(content).toHaveAttribute("aria-label", "Configuration JSON");

    rerender(
      <JsonEditor
        id="configuration-json"
        ariaLabel="Configuration JSON"
        value="{}"
        onChange={secondOnChange}
        height={120}
      />,
    );

    const currentContent = container.querySelector(".cm-content");
    const currentView = EditorView.findFromDOM(currentContent as HTMLElement);
    expect(currentView).toBe(originalView);
    expect(container.querySelector("#configuration-json")).toHaveStyle({
      height: "120px",
    });

    act(() => {
      currentView?.dispatch({
        changes: {
          from: 0,
          to: currentView.state.doc.length,
          insert: '{"changed":true}',
        },
      });
    });

    expect(firstOnChange).not.toHaveBeenCalled();
    expect(secondOnChange).toHaveBeenLastCalledWith('{"changed":true}');
  });

  it("keeps the cursor near the edited region when external normalization adds text", async () => {
    const original = '{\n  "a": 1,\n  "b": 2\n}';
    const normalized = '{\n  "a": 1,\n  "added": true,\n  "b": 2\n}';
    const { container, rerender } = await renderEditor({
      value: original,
      onChange: vi.fn(),
    });
    const view = EditorView.findFromDOM(
      container.querySelector(".cm-content") as HTMLElement,
    );
    const originalCursor = original.indexOf('"b"') + 1;

    act(() => {
      view?.dispatch({ selection: { anchor: originalCursor } });
    });
    rerender(<JsonEditor value={normalized} onChange={vi.fn()} />);

    expect(view?.state.selection.main.head).toBe(normalized.indexOf('"b"') + 1);
  });

  it("keeps the cursor on unchanged context between separate external edits", async () => {
    const original = '{"a":1,"b":2,"c":3}';
    const normalized = '{"a":100,"b":2,"c":300}';
    const { container, rerender } = await renderEditor({
      value: original,
      onChange: vi.fn(),
    });
    const view = EditorView.findFromDOM(
      container.querySelector(".cm-content") as HTMLElement,
    );
    const originalCursor = original.indexOf('"b"') + 1;

    act(() => {
      view?.dispatch({ selection: { anchor: originalCursor } });
    });
    rerender(<JsonEditor value={normalized} onChange={vi.fn()} />);

    expect(view?.state.selection.main.head).toBe(normalized.indexOf('"b"') + 1);
  });
});
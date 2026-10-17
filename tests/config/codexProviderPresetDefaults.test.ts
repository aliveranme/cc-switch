import { describe, expect, it } from "vitest";
import {
  codexProviderPresets,
  generateThirdPartyConfig,
} from "@/config/codexProviderPresets";
import { parse as parseToml } from "smol-toml";
import { extractCodexModelName } from "@/utils/providerConfigUtils";

describe("Codex provider defaults", () => {
  it("uses GPT-5.6 Sol for newly generated third-party providers", () => {
    const config = generateThirdPartyConfig(
      "example",
      "https://api.example.com/v1",
    );

    expect(extractCodexModelName(config)).toBe("gpt-5.6-sol");
  });

  it("keeps E-FlowCode's automatic compaction threshold within its context window", () => {
    const preset = codexProviderPresets.find(
      (candidate) => candidate.name === "E-FlowCode",
    );

    expect(preset).toBeDefined();
    const parsed = parseToml(preset?.config ?? "") as {
      model_context_window?: number;
      model_auto_compact_token_limit?: number;
    };
    const {
      model_context_window: contextWindow,
      model_auto_compact_token_limit: compactLimit,
    } = parsed;

    expect(contextWindow).toBe(1_000_000);
    expect(compactLimit).toBe(900_000);
    expect(compactLimit).toBeLessThan(contextWindow!);
    // 上游 v4.0.14（be0a9cf0）把所有预设里的 GPT 窗口按 272K 封顶：
    // 中转站的 GPT 多从 ChatGPT 订阅分发，按订阅窗口卡，写大只会让客户端
    // 不压缩直到上游拒收。TOML 的 model_context_window 上游自己仍是 1M，
    // fork 跟随；只有 modelCatalog（fork 侧 4.24 保留的字段）按新上限写。
    expect(preset?.modelCatalog).toEqual([
      {
        model: "gpt-5.5",
        displayName: "GPT-5.5",
        contextWindow: 272_000,
        supportsParallelToolCalls: undefined,
        inputModalities: undefined,
        baseInstructions: undefined,
      },
    ]);
  });
});

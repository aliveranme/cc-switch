import { describe, expect, it } from "vitest";
import {
  createUniversalProviderFromPreset,
  universalProviderPresets,
} from "@/config/universalProviderPresets";

describe("universal provider presets", () => {
  it("uses gpt-5.6-sol for newly created Codex providers", () => {
    for (const preset of universalProviderPresets) {
      const provider = createUniversalProviderFromPreset(
        preset,
        `${preset.providerType}-id`,
        "https://gateway.example/v1",
        "test-key",
      );

      expect(provider.models.codex?.model).toBe("gpt-5.6-sol");
    }
  });
});

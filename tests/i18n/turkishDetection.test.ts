import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

let loadedI18n:
  | { changeLanguage: (language: string) => Promise<unknown> }
  | undefined;

// fork 的 i18n 按需加载：语言包不在模块顶层初始化，必须显式 await initI18n()
// （上游是 eager 版本，import 即完成 init；见 src/i18n/index.ts）
async function loadInitializedI18n() {
  const mod = await import("@/i18n");
  await mod.initI18n();
  return mod.default;
}

beforeEach(() => {
  vi.resetModules();
  localStorage.clear();
});
afterEach(async () => {
  await loadedI18n?.changeLanguage("zh");
  loadedI18n = undefined;
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("Turkish language detection", () => {
  it.each(["tr", "tr-TR"])("detects %s on first launch", async (language) => {
    vi.stubGlobal("navigator", { language });
    const i18n = await loadInitializedI18n();
    loadedI18n = i18n;
    expect(i18n.language).toBe("tr");
    expect(i18n.t("common.save")).toBe("Kaydet");
  });
  it.each(["de-DE", "fr", "ko-KR"])(
    "uses English for unsupported system language %s",
    async (language) => {
      vi.stubGlobal("navigator", { language });
      const i18n = await loadInitializedI18n();
      loadedI18n = i18n;
      expect(i18n.language).toBe("en");
    },
  );
  it("keeps a saved language instead of overriding it with the system language", async () => {
    localStorage.setItem("language", "en");
    vi.stubGlobal("navigator", { language: "tr-TR" });
    const i18n = await loadInitializedI18n();
    loadedI18n = i18n;
    expect(i18n.language).toBe("en");
  });
});

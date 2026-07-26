import i18n from "i18next";
import { initReactI18next } from "react-i18next";

type Language = "zh" | "zh-TW" | "en" | "ja";

const DEFAULT_LANGUAGE: Language = "zh";
const FALLBACK_LANGUAGE: Language = "en";

// Each locale is 150-200 kB of JSON. Importing all four eagerly put 672 kB of
// translations into the main chunk, almost all of it for languages the user
// will never switch to. They are fetched per language instead.
const localeLoaders: Record<Language, () => Promise<{ default: object }>> = {
  en: () => import("./locales/en.json"),
  ja: () => import("./locales/ja.json"),
  zh: () => import("./locales/zh.json"),
  "zh-TW": () => import("./locales/zh-TW.json"),
};

const loadedLanguages = new Set<Language>();

const isLanguage = (value: string): value is Language =>
  value === "zh" || value === "zh-TW" || value === "en" || value === "ja";

async function loadLanguage(lng: Language): Promise<void> {
  if (loadedLanguages.has(lng)) return;
  const resource = await localeLoaders[lng]();
  i18n.addResourceBundle(lng, "translation", resource.default, true, true);
  loadedLanguages.add(lng);
}

const getInitialLanguage = (): Language => {
  if (typeof window !== "undefined") {
    try {
      const stored = window.localStorage.getItem("language");
      if (
        stored === "zh" ||
        stored === "zh-TW" ||
        stored === "en" ||
        stored === "ja"
      ) {
        return stored;
      }
    } catch (error) {
      console.warn("[i18n] Failed to read stored language preference", error);
    }
  }

  const navigatorLang =
    typeof navigator !== "undefined"
      ? (navigator.language?.toLowerCase() ??
        navigator.languages?.[0]?.toLowerCase())
      : undefined;

  if (navigatorLang === "zh") {
    return "zh";
  }

  if (
    navigatorLang?.startsWith("zh-tw") ||
    navigatorLang?.startsWith("zh-hk") ||
    navigatorLang?.startsWith("zh-mo") ||
    navigatorLang?.startsWith("zh-hant")
  ) {
    return "zh-TW";
  }

  if (navigatorLang?.startsWith("zh")) {
    return "zh";
  }

  if (navigatorLang?.startsWith("ja")) {
    return "ja";
  }

  if (navigatorLang?.startsWith("en")) {
    return "en";
  }

  return DEFAULT_LANGUAGE;
};

/**
 * 初始化 i18n 并加载所需语言包。
 *
 * 必须在渲染 App 之前 await，否则首帧会落到 key 上。只加载当前语言和
 * fallback 语言，其余语言在用户切换时才拉取。
 */
export async function initI18n(): Promise<void> {
  const lng = getInitialLanguage();

  await i18n.use(initReactI18next).init({
    resources: {},
    lng, // 根据本地存储或系统语言选择默认语言
    fallbackLng: FALLBACK_LANGUAGE, // 如果缺少当前语言的翻译则退回英文

    interpolation: {
      escapeValue: false, // React 已经默认转义
    },

    // 开发模式下显示调试信息
    debug: false,
  });

  await Promise.all(
    lng === FALLBACK_LANGUAGE
      ? [loadLanguage(lng)]
      : [loadLanguage(lng), loadLanguage(FALLBACK_LANGUAGE)],
  );
}

/**
 * 切换语言，必要时先拉取对应语言包。
 * 替代直接调用 i18n.changeLanguage —— 未加载的语言会整页退回英文。
 */
export async function ensureLanguage(lng: string): Promise<void> {
  if (!isLanguage(lng)) return;
  await loadLanguage(lng);
  await i18n.changeLanguage(lng);
}

export default i18n;

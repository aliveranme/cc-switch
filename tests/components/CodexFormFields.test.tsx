import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";
import type { FetchedModel } from "@/lib/api/model-fetch";

const modelFetchApiMock = vi.hoisted(() => ({
  fetchModelsForConfig: vi.fn(),
  fetchXaiOauthModels: vi.fn(),
  modelFetchRequestHeaders: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
  fetchXaiOauthModels: modelFetchApiMock.fetchXaiOauthModels,
  modelFetchRequestHeaders: modelFetchApiMock.modelFetchRequestHeaders,
  showFetchModelsError: modelFetchApiMock.showFetchModelsError,
}));

// 上游 v4.0.0 起 CodexFormFields 内部使用 useModelMetadataFill（依赖
// QueryClientProvider）；本文件只测 model fetch 行为，mock 成 no-op 即可。
vi.mock("@/hooks/useModelMetadataFill", () => ({
  useModelMetadataFill: () => () => {},
}));

type CodexFormFieldsProps = ComponentProps<typeof CodexFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();

  return <Form {...form}>{children}</Form>;
};

// RTL's default whitespace normalizer collapses the multi-line placeholder,
// so query by a distinctive fragment instead of the exact string.
const HEADERS_PLACEHOLDER = /X-Provider/;

// The harness owns the headers override state so editing the textarea
// reproduces the exact user flow: an existing header override is changed
// while Base URL / API Key stay untouched.
const INITIAL_HEADERS_OVERRIDE = '{"X-Tenant": "tenant-1"}';

function renderCodexForm() {
  const baseProps: CodexFormFieldsProps = {
    codexApiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    codexBaseUrl: "https://api.example.com",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "",
    onModelChange: vi.fn(),
    apiFormat: "openai_responses",
    onApiFormatChange: vi.fn(),
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    // fork 的额外必填 props（上游此测试没有它们；构造默认值即可，与
    // CodexFormFields.modelFetch.test.tsx 的构造方式一致）
    sessionAffinityHeader: "",
    onSessionAffinityHeaderChange: vi.fn(),
    codexNativeResponsesTemplate: "full",
    onCodexNativeResponsesTemplateChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    shouldShowSpeedTest: true,
  };

  function Harness() {
    const [headersOverride, setHeadersOverride] = useState(
      INITIAL_HEADERS_OVERRIDE,
    );

    return (
      <FormShell>
        <CodexFormFields
          {...baseProps}
          localProxyHeadersOverride={headersOverride}
          onLocalProxyHeadersOverrideChange={setHeadersOverride}
        />
      </FormShell>
    );
  }

  return render(<Harness />);
}

async function fetchAndSettleModels(models: FetchedModel[]) {
  modelFetchApiMock.fetchModelsForConfig.mockResolvedValueOnce(models);

  fireEvent.click(
    screen.getByRole("button", { name: "providerForm.fetchModels" }),
  );

  await waitFor(() => {
    expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalled();
  });

  return screen.findByRole("button", { name: "Select model" });
}

function changeHeadersOverride(value: string) {
  fireEvent.change(screen.getByPlaceholderText(HEADERS_PLACEHOLDER), {
    target: { value },
  });
}

describe("CodexFormFields model fetch identity", () => {
  beforeEach(() => {
    modelFetchApiMock.fetchModelsForConfig.mockReset();
    modelFetchApiMock.showFetchModelsError.mockReset();
  });

  it("修改 Header 覆盖后清空已取到的模型列表", async () => {
    renderCodexForm();

    // Header 覆盖已有初值，高级区自动展开，textarea 可直接编辑
    await fetchAndSettleModels([{ id: "model-a", ownedBy: "vendor" }]);
    expect(
      screen.getByRole("button", { name: "Select model" }),
    ).toBeInTheDocument();

    changeHeadersOverride('{"X-Tenant": "tenant-2"}');

    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Select model" })).toBeNull();
    });
  });

  it("Header 覆盖在途变更后丢弃旧请求返回的模型", async () => {
    renderCodexForm();

    let resolveFetch!: (models: FetchedModel[]) => void;
    modelFetchApiMock.fetchModelsForConfig.mockImplementationOnce(
      () =>
        new Promise<FetchedModel[]>((resolve) => {
          resolveFetch = resolve;
        }),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() => {
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalled();
    });

    changeHeadersOverride('{"X-Tenant": "tenant-2"}');

    // 旧身份的响应此时才落地。先正向等待响应链走完（按钮恢复可用），
    // 再断言模型未上榜——否则否定断言会在响应落地前抢先通过。
    resolveFetch([{ id: "stale-model", ownedBy: "vendor" }]);

    const fetchButton = screen.getByRole("button", {
      name: "providerForm.fetchModels",
    });
    await waitFor(() => {
      expect(fetchButton).toBeEnabled();
    });

    expect(screen.queryByRole("button", { name: "Select model" })).toBeNull();
  });
});

function renderCustomCodexForm(overrides: Partial<CodexFormFieldsProps> = {}) {
  const props: CodexFormFieldsProps = {
    codexApiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    codexBaseUrl: "https://api.example.com",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "",
    onModelChange: vi.fn(),
    apiFormat: "openai_responses",
    onApiFormatChange: vi.fn(),
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    codexChatReasoning: {},
    onCodexChatReasoningChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    sessionAffinityHeader: "",
    onSessionAffinityHeaderChange: vi.fn(),
    codexNativeResponsesTemplate: "full",
    onCodexNativeResponsesTemplateChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    shouldShowSpeedTest: true,
    ...overrides,
  };

  return render(
    <FormShell>
      <CodexFormFields {...props} />
    </FormShell>,
  );
}

describe("CodexFormFields fork advanced fields", () => {
  it("auto-expands advanced options when sessionAffinityHeader is set and shows NativeResponses template in openai_responses mode", () => {
    renderCustomCodexForm({
      apiFormat: "openai_responses",
      sessionAffinityHeader: "x-session-affinity",
    });

    expect(screen.getByLabelText("会话亲和 Header")).toHaveValue(
      "x-session-affinity",
    );
    expect(
      screen.getByLabelText("NativeResponses 目录模板"),
    ).toBeInTheDocument();
    expect(screen.queryByText("思考能力")).toBeNull();
  });

  it("auto-expands advanced options when codexNativeResponsesTemplate is neutral", () => {
    renderCustomCodexForm({
      apiFormat: "openai_responses",
      codexNativeResponsesTemplate: "neutral",
    });

    expect(
      screen.getByLabelText("NativeResponses 目录模板"),
    ).toBeInTheDocument();
    expect(screen.getByText("中性（兼容优先）")).toBeInTheDocument();
  });

  it("hides NativeResponses template in openai_chat mode while keeping sessionAffinityHeader and reasoning fields", () => {
    renderCustomCodexForm({
      apiFormat: "openai_chat",
      sessionAffinityHeader: "x-session-affinity",
    });

    expect(screen.getByLabelText("会话亲和 Header")).toHaveValue(
      "x-session-affinity",
    );
    expect(screen.queryByLabelText("NativeResponses 目录模板")).toBeNull();
    expect(screen.getByText("思考能力")).toBeInTheDocument();
  });

  it("exposes NativeResponses template and sessionAffinityHeader in stack variant", () => {
    const { unmount } = renderCustomCodexForm({
      variant: "stack",
      apiFormat: "openai_responses",
      sessionAffinityHeader: "x-stack-affinity",
      codexNativeResponsesTemplate: "neutral",
    });

    expect(screen.getByLabelText("会话亲和 Header")).toHaveValue(
      "x-stack-affinity",
    );
    expect(
      screen.getByLabelText("NativeResponses 目录模板"),
    ).toBeInTheDocument();
    unmount();

    renderCustomCodexForm({
      variant: "stack",
      apiFormat: "openai_chat",
      sessionAffinityHeader: "x-stack-chat",
    });

    expect(screen.getByLabelText("会话亲和 Header")).toHaveValue(
      "x-stack-chat",
    );
    expect(screen.queryByLabelText("NativeResponses 目录模板")).toBeNull();
    expect(screen.getByText("思考能力")).toBeInTheDocument();
  });
});

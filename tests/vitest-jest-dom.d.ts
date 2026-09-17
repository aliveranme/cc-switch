// vitest 5 把 Assertion 的签名从 `Assertion<T = any>` 改成 `Assertion<R, T>`（两个
// 必需类型参数）。@testing-library/jest-dom 7.0.1 自带的 types/vitest.d.ts 仍声明为
// `interface Assertion<T = any>`，类型参数列表不一致使 TypeScript 的接口声明合并
// **静默失效**——jest-dom 的匹配器（toBeInTheDocument / toBeDisabled /
// toHaveTextContent …）全部从 Assertion 上消失，`pnpm typecheck` 报 600+ 处 TS2339
// （vitest 5 的 CI 失败即为此故，与测试运行时无关）。
//
// 这里按 vitest 5 的签名补齐；参数含义对照 jest-dom 自己的 types/jest.d.ts：
// 第二个类型参数是匹配器的返回值类型，E 保留 any（与上游写法一致）。
// jest-dom 上游适配 vitest 5 后可删除本文件。
import type { TestingLibraryMatchers } from "@testing-library/jest-dom/matchers";

declare module "vitest" {
  interface Assertion<R, T> extends TestingLibraryMatchers<any, R> {}
  interface AsymmetricMatchersContaining
    extends TestingLibraryMatchers<any, any> {}
}
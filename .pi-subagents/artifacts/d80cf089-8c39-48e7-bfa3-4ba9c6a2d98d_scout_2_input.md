# Task for scout

Analiza la salud general, configuración y ecosistema del proyecto cc-switch en /Volumes/ccc/Projects/cc-switch. Enfoque:

1. **Configuración del proyecto**: package.json (dependencias, scripts), tsconfig.json, vite.config.ts, vitest.config.ts, postcss.config.cjs, tailwind.config.cjs, pnpm-workspace.yaml.
2. **Build system**: Tauri config (src-tauri/tauri.conf.json), build.rs, Rust toolchain (rust-toolchain.toml).
3. **Testing**: examina tests/, setup files, cobertura de tests.
4. **Plugins y extensiones**: revisa la carpeta plugins/, assets/, scripts/.
5. **Documentación**: README.md, CHANGELOG.md, docs/.
6. **Configuración del agente**: .claude/, .pi/, .codebuddy/, .workbuddy/, .agents/, .atl/.
7. **Componentes externos**: CLIProxyAPI/, claude-code-reverse/, codex/, chatgpt-desktop-analysis/, ampcode-analysis/.
8. **GitHub ecosystem**: .github/, LICENSE, CODE_OF_CONDUCT.md, CONTRIBUTING.md, SECURITY.md, SUPPORT.md.

Devuelve un análisis estructurado con:
- Stack tecnológico completo
- Estado del proyecto (versión, madurez)
- Cobertura de tests y calidad
- Dependencias clave
- Riesgos o áreas de mejora
- Recomendaciones generales

---
Update progress at: /Volumes/ccc/Projects/cc-switch/.pi-subagents/artifacts/progress/d80cf089-8c39-48e7-bfa3-4ba9c6a2d98d/progress.md

---
**Output:**
Write your findings to exactly this path: /Volumes/ccc/Projects/cc-switch/.pi-subagents/artifacts/outputs/d80cf089-8c39-48e7-bfa3-4ba9c6a2d98d/context.md
This path is authoritative for this run.
Ignore any other output filename or output path mentioned elsewhere, including output destinations in the base agent prompt, system prompt, or task instructions.

## Acceptance Contract
Acceptance level: reviewed
Completion is not accepted from prose alone. End with a structured acceptance report.

Criteria:
- criterion-1: Implement the requested change without widening scope
- criterion-2: Return evidence sufficient for an independent acceptance review

Required evidence: changed-files, tests-added, commands-run, validation-output, residual-risks, no-staged-files

Review gate: required by reviewer.

Finish with a fenced JSON block tagged `acceptance-report` in this shape:
Use empty arrays when no items apply; array fields contain strings unless object entries are shown.
```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "specific proof"
    }
  ],
  "changedFiles": [
    "src/file.ts"
  ],
  "testsAddedOrUpdated": [
    "test/file.test.ts"
  ],
  "commandsRun": [
    {
      "command": "command",
      "result": "passed",
      "summary": "short result"
    }
  ],
  "validationOutput": [
    "validation output or concise summary"
  ],
  "residualRisks": [
    "none"
  ],
  "noStagedFiles": true,
  "diffSummary": "short description of the diff",
  "reviewFindings": [
    "blocker: file.ts:12 - issue found, or no blockers"
  ],
  "manualNotes": "anything else the parent should know"
}
```
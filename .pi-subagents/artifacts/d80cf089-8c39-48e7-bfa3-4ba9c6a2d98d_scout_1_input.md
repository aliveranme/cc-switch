# Task for scout

Analiza el frontend React/TypeScript del proyecto cc-switch en /Volumes/ccc/Projects/cc-switch. Enfoque:

1. **Estructura de componentes**: examina src/components/, especialmente los componentes UI en src/components/ui/ (shadcn/ui basado en components.json).
2. **Sistema de configuración**: analiza src/config/ — todos los presets de providers (claude, codex, gemini, hermes, opencode, openclaw, universal), templates, constantes, appConfig.
3. **Routing y estado**: examina src/App.tsx, src/contexts/, src/hooks/ para entender el manejo de estado.
4. **Tipos**: src/types.ts, src/types/ para entender el modelo de datos.
5. **Utilidades**: src/utils/ para formateo, manejo de errores, utilidades de providers.
6. **Internacionalización**: src/i18n/.

Devuelve un análisis estructurado con:
- Resumen general de la arquitectura frontend
- Stack tecnológico (React, TypeScript, Tailwind, shadcn/ui, etc.)
- Sistema de componentes y diseño
- Manejo de estado y flujo de datos
- Áreas de mejora o riesgo
- Tamaño y complejidad estimados

---
Update progress at: /Volumes/ccc/Projects/cc-switch/.pi-subagents/artifacts/progress/d80cf089-8c39-48e7-bfa3-4ba9c6a2d98d/progress.md

---
**Output:**
Write your findings to exactly this path: /Volumes/ccc/Projects/cc-switch/.pi-subagents/artifacts/outputs/d80cf089-8c39-48e7-bfa3-4ba9c6a2d98d/context.md
This path is authoritative for this run.
Ignore any other output filename or output path mentioned elsewhere, including output destinations in the base agent prompt, system prompt, or task instructions.

## Acceptance Contract
Acceptance level: attested
Completion is not accepted from prose alone. End with a structured acceptance report.

Criteria:
- criterion-1: Return concrete findings with file paths and severity when applicable

Required evidence: review-findings, residual-risks

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
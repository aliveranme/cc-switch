# Task for scout

Analiza en profundidad el backend Rust/Tauri del proyecto cc-switch en /Volumes/ccc/Projects/cc-switch. Enfoque:

1. **Arquitectura del proxy** (src-tauri/src/proxy/): examina la estructura de proxy/, cómo se enrutan las peticiones, los providers (claude.rs, codex.rs, gemini.rs), el sistema de forwarder, el mapeo de modelos, el clasificador y el manejo de SSE.
2. **Session Manager** (src-tauri/src/session_manager/): cómo maneja sesiones multi-provider (claude, codex, gemini, hermes, opencode, openclaw).
3. **Base de datos** (src-tauri/src/database/): esquema, migraciones, DAOs.
4. **MCP system** (src-tauri/src/mcp/ y claude_mcp.rs, gemini_mcp.rs): cómo implementa el protocolo MCP.
5. **Configuración y providers** (provider.rs, provider_defaults.rs, config.rs, app_config.rs).

Devuelve un análisis estructurado con:
- Resumen general de la arquitectura backend
- Componentes principales y sus responsabilidades
- Patrones de diseño identificados
- Flujo de datos típico de una petición
- Áreas de posible mejora o riesgo
- Tamaño y complejidad estimados del código Rust

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
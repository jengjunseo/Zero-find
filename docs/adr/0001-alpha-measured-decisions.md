# ADR 0001 — Alpha decisions supported by measurements

Status: accepted for Alpha, 2026-09-04.

## Search

Use normalized filenames plus flat substring scan and a fixed Top-20 heap. The optimized baseline met the provisional 5 ms P99 target at 100K fixture entries but not at 1M. A selective index is deferred until a representative real namespace confirms the 1M bottleneck; the benchmark harness is the comparison gate.

## Ancestor traversal

Do not add unconditional query-local HashMap memoization. It helps deep, highly shared paths and hurts mixed/unrelated paths enough to require evidence-based selection if ACL traversal is later implemented.

## UI

Use Rust plus raw Win32/GDI/DWM for Alpha. The measured resident process was about 15.2 MiB after a rendered search, the packaged executable was about 415 KiB, and the required global-hotkey/floating-island interaction fits the native message loop without a browser runtime.

## MFT bootstrap

No decision yet. The code path exists, but the current non-elevated session returned access denied. Directory traversal under the user's own token is used by the Alpha product so namespace discovery does not bypass access checks.



# ZeroFind stable contract

- ZeroFind searches a prebuilt in-memory file map; the query hot path does not traverse or read files.
- Optimize only after release-mode measurement. Flat scan is the baseline until evidence rejects it.
- Default ranking is file/folder first and demotes installers, executables, and shortcuts.
- Runtime object identity is not user identity (pins, usage, or prewarm intent).
- Namespace visibility fails closed. Product indexing only walks paths visible to the current user.
- Initial scope is Windows 10/11 x64 and local fixed NTFS volumes. Content, web, semantic, OCR, and network-drive search are out of scope.
- Benchmark evidence lives in `docs/benchmarks`; decisions based on evidence live in `docs/adr`.
- The native EDIT owns text editing and IME composition. Do not replace it with WM_CHAR string concatenation, and do not intercept Enter while an IME owns it.
- Never call reentrant Win32 control or window APIs while holding the APP mutex. Snapshot state, release the lock, then call Windows.
- Directory notifications are hints: reconcile using the current user's visible filesystem state. Watch before bootstrap; rebuild after overflow; do not follow junctions/reparse directories.
- `scripts/certify.ps1` exercises the actual executable with isolated fixtures. Its optional GDI paint-buffer image excludes native EDIT and DWM composition; use actual window inspection to certify the default Direct2D renderer.



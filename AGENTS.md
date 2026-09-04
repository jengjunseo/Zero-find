# ZeroFind stable contract

- ZeroFind searches a prebuilt in-memory file map; the query hot path does not traverse or read files.
- Optimize only after release-mode measurement. Flat scan is the baseline until evidence rejects it.
- Default ranking is file/folder first and demotes installers, executables, and shortcuts.
- Runtime object identity is not user identity (pins, usage, or prewarm intent).
- Namespace visibility fails closed. Product indexing only walks paths visible to the current user.
- Initial scope is Windows 10/11 x64 and local fixed NTFS volumes. Content, web, semantic, OCR, and network-drive search are out of scope.
- Benchmark evidence lives in `docs/benchmarks`; decisions based on evidence live in `docs/adr`.



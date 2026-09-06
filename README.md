# ZeroFind

A resident Windows file finder: invoke, type a filename, choose, open. The current candidate combines a small native blue island, an in-memory search map, and user-token directory notifications.

## Build and run

Requires Windows 10/11 x64 and Rust stable with the x86_64-pc-windows-gnu toolchain. The build script accepts Cargo on PATH or the repository-local .tools toolchain. It does not install Rust or alter your system PATH.

~~~powershell
.\scripts\build.ps1
.\dist\ZeroFind.exe
~~~

The executable stays resident. The factory shortcut is Ctrl+Shift+Space; legacy factory Ctrl+Alt+Space settings migrate on load. It indexes visible local fixed NTFS roots by default. Indexing publishes searchable batches; later create/rename/delete notifications update the map without filesystem reads in the search loop. Overflow requests a fresh scan. No privileged service, persistent index, content scanning, or network search is used.

## Interaction

| Action | Key / gesture |
|---|---|
| Expand / collapse | Ctrl+Shift+Space; click the compact island to expand |
| Edit query | Native Windows editing, IME, clipboard, Ctrl+A |
| Select result | Up / Down; selection scrolls into view |
| Scroll | Mouse wheel over the island |
| Open | Enter or double-click a row |
| Open containing folder | Ctrl+Enter |
| Collapse | Escape or switch to another application |
| Change global hotkey | F2; Escape cancels |
| Refresh file map | F5 |
| Exit resident process | Alt+F4 |

The compact island retains its position. Initial invocation and reopening after an open action use the foreground monitor. Drag the narrow top margin to reposition it. Result height is bounded by the monitor work area. Motion reverses toward the latest state; Windows' disabled client-area animation preference is respected.

The first result gets space immediately. Up to 100 ranked results are retained, with an internally scrolled viewport. Names and paths precede asynchronously fetched modified dates and sizes. During a newer query, older rows can remain visible to prevent layout jumping; open actions wait for the current result set. The footer reports a top-result limit, not the total number of matching files.

## Scope for diagnostics

~~~powershell
$env:ZEROFIND_SCAN_ROOTS = 'C:\Your\TestFolder'
.\dist\ZeroFind.exe
~~~

An explicitly empty or invalid scope does **not** fall back to scanning all drives. Reparse-point directories are not traversed. Settings use a flushed temporary file and Windows replacement semantics; a failed save is reported.

## Verify

~~~powershell
.\scripts\certify.ps1
# With your configured Cargo environment:
cargo test --release --target x86_64-pc-windows-gnu
cargo clippy --all-targets --target x86_64-pc-windows-gnu -- -D warnings
.\target\x86_64-pc-windows-gnu\release\bench_compare.exe 100000 100
.\target\x86_64-pc-windows-gnu\release\bench_compare.exe 1000000 100
~~~

Certification writes a report under artifacts using a newly generated fixture. With ZEROFIND_RENDERER=gdi it can also emit a renderer-only BMP. It never opens user files. Set ZEROFIND_DIAGNOSTIC_WINDOW=1 to expose a taskbar window for external UI tooling. Normal runs remain tool windows.

The original spike_a, spike_b and spike_c experiments remain available. The new paired benchmark alternates standard substring matching and a reused memchr finder with identical ranking, data, and Top-K limits; it asserts result equivalence.

## Candidate status

See [current measurements and certification debt](docs/benchmarks/2026-09-05-candidate.md) and [the architecture decision](docs/adr/0002-native-island-and-live-map.md).

This is a tested implementation candidate, not a certified 1.0 release. The actual Direct2D window, Korean Unicode input, keyboard selection, collapse/reopen, Ctrl+Shift+Space from another application, and opening a test text document were verified. Physical IME composition, multi-monitor transitions, and a fair Windows Search comparison remain unverified. Direct2D/DirectWrite is the default; set ZEROFIND_RENDERER=gdi to force the readable fallback. A successful DWM request alone is not a blur guarantee. There are type badges, not shell thumbnails. Large namespaces still require a cold traversal and linear search; there is no million-file latency guarantee.

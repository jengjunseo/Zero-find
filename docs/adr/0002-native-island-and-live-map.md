# ADR 0002 — Native island and live user-visible map

Accepted for the implementation candidate, 2026-09-06. This does not certify a 1.0 release.

## Search: reuse preprocessing, keep the flat control

The original archive at 5a2c10c and an alternating paired harness show that filename search repeatedly paid for substring-search setup. Use one memchr Finder per nonempty query. UTF-8 match offsets remain identical to str::find for a nonempty UTF-8 needle; multilingual differential tests and paired result assertions cover that contract. See [the library contract](https://docs.rs/memchr/2.8.3/memchr/memmem/index.html).

Paired Top-100 median search latency was 2.44–3.57 ms at 100K and 24.35–34.30 ms at 1M. The identical-ranking std-search control took 5.04–7.37 ms and 53.27–73.59 ms respectively. Long-tail interruptions remain: this is not a sub-5ms P99 architecture at 1M. A postings index was not justified by a representative real million-file namespace in this session. No additional per-entry search structure was introduced; static character-length cost folds into the existing rank bias.

The Top-K heap and final sort now share deterministic name/path tie-breaking. UI results are bounded at 100; the original spike remains Top-20 for archival comparisons. These are distinct workloads, never presented as equivalent.

## Input and rendering

Native EDIT owns text editing, selection, clipboard, UTF-16 input, and IME composition. The subclass leaves composition-owned Enter/arrow handling alone. Buffered painting explicitly repairs alpha for that child control on glass.

Use retained Direct2D and DirectWrite resources for the island. In live inspection, the original GDI/full-frame DWM combination erased content. GDI with zero glass margins was readable but allowed distracting background text through its constant-alpha surface. The Direct2D candidate resolved alpha and text independently; the native EDIT needed an additional buffered-alpha fix, verified in the actual window. Its measured small-fixture working set was about 48 MiB versus about 29 MiB for the GDI candidate, and one executable run painted first results in 9.3 ms versus a 28–54 ms range observed in GDI development runs. These are individual observations, not a statistical renderer benchmark.

The extra native graphics cost buys the requested material/text quality without a browser runtime. Device/render failure switches to readable GDI with glass margins removed. Windows 10 may decline the Windows 11 system-backdrop request. Never infer a blur guarantee from a successful API call. See [Microsoft's render-target lifetime guidance](https://learn.microsoft.com/en-us/windows/win32/api/d2d1/nf-d2d1-id2d1factory-createhwndrendertarget%28constd2d1_render_target_properties_constd2d1_hwnd_render_target_properties_id2d1hwndrendertarget%29).

windows-rs 0.58 is intentional: its import libraries build with the repository's self-contained GNU toolchain. The 0.62 raw-dylib route required an external assembler in this environment and offered no necessary API capability for this renderer.

Geometry is elapsed-time-based and retargetable. Timers stop when settled. The first row receives enough space immediately; remaining height converges to its target. This is Win32 geometry animation, not DirectComposition transform animation. Do not claim compositor-only motion or frame-pacing certification.

## Indexing and permissions

Keep one process and user-token directory traversal. Arm overlapped ReadDirectoryChangesW before bootstrap, stream 10K-entry batches, and reconcile notifications against visible filesystem state. Rename removes old subtree paths and scans the new visible subtree. Notification buffers are bounds-checked; overflow replaces the affected root. Directory handles wait without busy polling; F5 requests a refresh. Unavailable roots are removed and retried.

No privileged raw MFT namespace is shared with the UI. Reject reparse directories before canonicalization and before traversal. Explicit invalid scopes stay empty. There is no persisted index, identity history, or pins to corrupt. Settings use a flushed temporary file and MoveFileEx replacement; runtime success and save success are reported separately.

## Known limits

Cold full-volume enumeration, volume-wide I/O interference, notification-overflow stress, ACL-revocation races, 8.3 notification aliases, full IME composition, multi-monitor physical transitions, shell opens over 260 characters, screen-reader result-list access, shell thumbnails, and a fair Windows Search comparison require further certification or work. Recovery snapshots are not yet implemented. The candidate is a stronger local finder, not a claim that every harness aspiration has shipped.

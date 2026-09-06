# ZeroFind implementation candidate — September 5–6, 2026

## Environment and controls

MEASURED on Windows 11 Pro 10.0.26200, AMD Ryzen 3 4100, approximately 16 GiB RAM, Rust 1.98.1 GNU x64, release/LTO. Storage media and a controlled cold disk-cache state were not established. This is a different machine from the historical September 4 Intel benchmark; those old numbers are not the current baseline.

The untouched source at commit 5a2c10ceb02b8beea0fed757f7612862cb9886b9 was rebuilt separately. Its original Top-20 spike results are retained under [raw evidence](2026-09-05-raw/). Intermediate development runs are also retained and are not called final results.

The decisive comparison below alternates std::str::find and one reused memchr Finder in the same process, on the same entries with the same deterministic ranking and Top-100 heap. There are 8 warm-up pairs and 80 measured pairs per query. The harness asserts equality of paths, scores and offsets. This isolates matcher preprocessing; its reference is **not** the untouched old executable.

## Paired search results (P50)

| Query | 100K: control → candidate | 1M: control → candidate |
|---|---:|---:|
| gauss | 5.99 → 3.57 ms | 58.39 → 34.30 ms |
| project | 5.89 → 3.23 ms | 59.58 → 33.38 ms |
| a | 5.33 → 2.80 ms | 54.80 → 29.14 ms |
| 1 | 5.33 → 2.60 ms | 56.49 → 29.92 ms |
| 가우스 | 5.30 → 3.05 ms | 54.39 → 30.63 ms |
| 문서 | 5.24 → 3.15 ms | 53.27 → 31.29 ms |
| annual_project_report | 7.37 → 2.44 ms | 73.59 → 24.35 ms |
| not-present | 5.56 → 2.73 ms | 58.57 → 30.16 ms |
| 0000999 | 5.04 → 2.90 ms | 56.79 → 33.44 ms |

Across these nine queries, median reductions were approximately 40–67%. No persistent postings structure or additional per-entry field was added. Synthetic fixture construction took about 59 ms at 100K and 559 ms at 1M; this is allocation/build of synthetic entries, **not disk indexing time**.

P99 is noisy and is not uniformly improved: at 100K the candidate's one-character a query reached 15.39 ms versus 6.15 ms for its paired control. At 1M candidate P99 ranged about 25.51–101.91 ms. Other desktop activity was not disabled. There is no P99 SLO certification and no 1M-in-5ms claim.

Reproduce with bench_compare 100000 100 and bench_compare 1000000 100 after a release build. The original spike_b remains Top-20 and must not be confused with this Top-100 UI workload.

## Executable and freshness certification

The executable runner uses 34 generated text files. It exercises native EDIT changes, 30 Korean matches, a single match, three matches, superseded queries, no results, selection of the 16th result, scroll visibility, collapse, interruption/reopen, compact geometry, working set, and a three-second settled interval. It exits the application at the end. [The final run](2026-09-05-raw/executable-certification.txt) and [test output](2026-09-05-raw/final-tests.txt) contain the exact observations.

Timestamps are query enqueue → CPU paint completion of a full first row. They are not keyboard hardware → monitor presentation timestamps. The first indexing timestamp starts after UI setup and is not cold process startup. A final Direct2D run is recorded separately from intermediate GDI runs; differences between individual GUI runs are not percentile estimates.

The real filesystem test creates, renames, deletes and moves only its own temporary fixture. One recorded run observed create→searchable 10.61 ms, rename→updated 11.08 ms and delete→absent 10.63 ms, using 10 ms polling. These are upper-bound observations at that polling resolution, not filesystem event precision. Overflow parsing is tested with malformed/truncated records; actual OS overflow and volume-removal stress remain unverified.

## Actual UI inspection

MEASURED/OBSERVED in the running desktop application:

- Korean Unicode text entered into the native EDIT and useful results displayed.
- Keyboard selection, Escape collapse, compact activation and global re-expansion.
- Ctrl+Shift+Space invocation while the test document was active in Notepad.
- Enter opened the generated 배포자료.txt in Notepad; ZeroFind hid afterward.
- Direct2D rows and native EDIT remained readable after repairing child-control alpha.
- The actual window region rejects corner (0,0) and includes an interior point.

A renderer-only BMP from earlier GDI development excluded the native child and DWM composition. It initially looked correct while the live window did not. Live inspection caught that discrepancy and caused the renderer change. Such images must never be used as proof of the final compositor output.

## Windows Search comparison

NOT VERIFIED. No fair same-machine, same-scope Windows Search flow comparison was completed. Do not claim that ZeroFind beats Windows Search. The measured claim is narrower: the candidate is faster than its identical-ranking string-matching control and now supports live updates and native file-finding interactions absent from the original prototype.

## Remaining certification debt

Cold full-volume readiness and peak memory; HDD/SSD I/O interference; sleep/resume and volume removal; actual overflow recovery under load; ACL changes during traversal; long-path shell opening; physical Korean IME composition; accessibility of the custom result list; monitor transitions and theme/high-contrast changes; frame pacing and compositor-present timing. Shell thumbnails and persistent cold-start snapshots are not implemented. Windows 10 backdrop fallback has not been visually tested on a Windows 10 device.

The candidate is buildable and exercised, but these gaps prevent calling it a fully certified 1.0 release.

Final executable: 606,720 bytes; SHA-256 7E1CC8DFCAF786E371621F5F1640A25C3F3BE1DE65F57C49D503910A2D5DB942. The final runner also checks process exit code zero; all 10 tests and Clippy with warnings denied passed. A prior graphics shutdown fault was corrected by explicitly releasing renderer and buffered-paint resources before exit. The final dist executable was additionally invoked from the test Notepad window using Ctrl+Shift+Space, with native EDIT focus observed.

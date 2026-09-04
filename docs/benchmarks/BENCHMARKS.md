# ZeroFind alpha benchmark evidence

Measured on 2026-09-04 (Asia/Seoul), release mode, Rust 1.98.1 GNU x64, Intel Core i5-12500, 16.9 GB physical RAM, Windows build 22631.6199. Storage media could not be queried from this sandbox. Raw structured results are in `bench-results.json`.

## MEASURED

### Spike B — flat in-memory filename scan

The first 100K run exposed a real implementation problem: common-query P99 reached 40.952 ms because every match allocated an extension string and the Top-20 selection linearly rescanned its candidates. The fix precomputes static rank bias and uses a fixed 20-element heap. This is still a flat substring scan; no trigram, FST, trie, or postings index was introduced.

| Dataset | P99 range across ASCII/Korean/short/long queries | Decision |
|---|---:|---|
| 100K | 2.342–4.766 ms | Flat scan meets the provisional 5 ms P99 target on this fixture. |
| 1M | 22.914–41.709 ms | Flat scan does not meet 5 ms. Keep the simpler alpha, measure a real 1M namespace, then compare the smallest selective index in the same harness. |

The measured values do not justify a claim that ZeroFind searches one million entries in 5 ms.

### Spike C — ancestor traversal

At realistic Top-20 counts and shallow depth, naive traversal was sub-microsecond and faster than HashMap memoization. Memoization helped only when many candidates shared very deep ancestry (for example depth 500/count 100: 22.0 µs versus 104.8 µs). It was dramatically worse for unrelated ancestry (2236.8 µs versus 118.5 µs). The alpha therefore does not add query-local ancestor memoization.

### Desktop alpha

- Release executable: 427,520 bytes.
- Working set after launch, indexing the repository, typing `zerofind`, and rendering results: 15,921,152 bytes.
- Visually certified: borderless rounded island, blue glass backdrop, query input, result list, selected-row hierarchy, and clean close.
- The full-screen certification capture was inspected locally and intentionally excluded from the public repository because it contained unrelated desktop context.

## NOT YET VERIFIED

Spike A compiled and executed, but opening `\\.\C:` returned Windows error 5 because this session was not elevated. There is deliberately no fabricated enumeration rate. Run an elevated shell and execute:

```powershell
.\target\x86_64-pc-windows-gnu\release\spike_a.exe C:
```

Cold/warm storage conditions, hotkey-to-first-visible-frame timing, full-drive index duration, and actual Enter-open behavior remain certification debt.

## Reproduction

```powershell
.\scripts\build.ps1
.\target\x86_64-pc-windows-gnu\release\spike_a.exe C:
.\target\x86_64-pc-windows-gnu\release\spike_b.exe 100000
.\target\x86_64-pc-windows-gnu\release\spike_b.exe 1000000
.\target\x86_64-pc-windows-gnu\release\spike_c.exe
```


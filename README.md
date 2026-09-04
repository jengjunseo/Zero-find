# ZeroFind

ZeroFind is a native, resident Windows file finder. It builds an in-memory map once, then answers each keystroke with a ranked flat scan: no filesystem traversal or content reads occur on the query hot path.

## Run the desktop app

The ready-to-run build is produced at `dist/ZeroFind.exe` by `scripts/build.ps1`.

```powershell
.\scripts\build.ps1
.\dist\ZeroFind.exe
```

- `Ctrl+Alt+Space`: show/hide the floating search island (default; configurable)
- `Up` / `Down`: move selection
- `Enter`: open the selected item
- `Ctrl+Enter`: open its containing folder
- `Esc`: hide the island while keeping ZeroFind resident
- `F2`: capture and save a new global hotkey
- `Alt+F4`: exit

On first launch, indexing runs in the background under the current user's Windows permissions. Set `ZEROFIND_SCAN_ROOTS` to a semicolon-separated list to constrain roots for diagnostics.

## Reproduce the spikes

```powershell
.\scripts\bench.ps1
```

- `spike_a C:`: direct `FSCTL_ENUM_USN_DATA` enumeration (may require elevation)
- `spike_b`: flat substring search latency
- `spike_c`: ancestor walking versus query-local memoization

The checked-in evidence and its limitations are in [docs/benchmarks/BENCHMARKS.md](docs/benchmarks/BENCHMARKS.md).

## Why raw Win32

The alpha deliberately uses Rust plus Win32/GDI/DWM directly. It stays resident, has no browser runtime, uses a global hotkey, and keeps the packaged executable small. The visual layer is a borderless rounded blue search island with native DWM backdrop and subtle opacity motion.



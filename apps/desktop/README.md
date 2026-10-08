# Desktop companion (Phase 0)

Tauri 2 backend (`src-tauri/`, Rust) + React+Vite UI (`src/`).

Pinned: Tauri `2.2.7`, `tauri-build 2.2.3`, React `18.3.1`, Vite `6.3.5`, TS `5.6.3`, Node `22.17.1 LTS` (root `.nvmrc`). Your shell has Node 24 — fine for Phase 0 web build; CI uses 22.

## Run web UI only (no Rust needed)

```powershell
cd apps/desktop
npm install
npm run build
```

## Full Tauri run (needs Rust 1.89.0 + WebView2 on Win10)

```powershell
cd apps/desktop
npm install
npx tauri dev
```

Real tray/service/injector wiring lands in Phase 1.

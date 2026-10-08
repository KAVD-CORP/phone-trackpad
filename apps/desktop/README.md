# Desktop companion (Tauri 2)

Tauri 2 backend (`src-tauri/`, Rust) + React+Vite UI (`src/`).

The desktop companion receives input and injects cursor movement and clicks into the OS via native adapters (`SendInput` on Windows, `CGEvent` on macOS). The webview inside Tauri handles UI, settings, and pairing—it does not directly invoke OS injection APIs.

Pinned: Tauri `2.2.7`, `tauri-build 2.2.3`, React `18.3.1`, Vite `6.3.5`, TS `5.6.3`, Node `22.17.1 LTS` (root `.nvmrc`).

## Run web UI only (no Rust needed)

```powershell
cd apps/desktop
npm install
npm run build
```

## Full Tauri run (needs Rust 1.89.0+ + WebView2 on Windows)

```powershell
cd apps/desktop
npm install
npx tauri dev
```

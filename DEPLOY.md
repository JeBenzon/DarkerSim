# Deploying Darker Sim

Darker Sim is a **static site**: Rust compiled to WebAssembly plus JSON data and icons. It has no backend and no database, and it needs no environment variables or secrets.

## The game data must be in place first

This repository contains **no game data**. Before you can build, these paths must exist in the working tree:

- `app/data/game.json`: the game database
- `data/icons/`: item, ability, buff, class and skin icons

`scripts/update.ps1` generates both from your own Dark and Darker install (see the README). Both paths are in `.gitignore`, so they never get committed here. If either is missing, the Docker build stops right away with an error.

## What's in the repo

- `sim/` holds the source of the engine (`dad-core`) and the web app (`dad-web`, Leptos CSR, built with trunk).
- `data/archetypes.json` holds the build presets (our own).
- `Dockerfile` builds in two stages: the first runs `trunk build --release`, the second serves the result with `nginx:alpine` on **port 80**.
- `deploy/nginx.conf` holds the gzip, cache and SPA fallback settings.

`trunk build` copies `app/data/game.json`, `data/archetypes.json` and `data/icons/` into `dist/`. The paths are set in `sim/crates/dad-web/index.html`.

## Building and running

With the game data in place:

```sh
docker build -t darkersim .
docker run --rm -p 8080:80 darkersim
```

On a platform such as Coolify, use the **Dockerfile** build pack at `/Dockerfile` with base directory `/` and exposed port **80**.

The first build takes about 5–10 minutes because it compiles the Rust crates and downloads trunk and wasm-bindgen. It needs internet access to GitHub (the trunk release) and crates.io.

To check it, open the site. The app should load the class picker and the gear grid, and `/game.json` should return JSON.

## DarkerLink (not part of the deployment)

DarkerLink (`tools/DarkerLink`) is a Windows tray app that runs on the player's PC. It is not built or deployed with the site, and the Dockerfile doesn't copy `tools/` at all. The site has a **DarkerLink** tab that explains what it does.

The page tries to connect to `http://localhost:47321` in the background. When DarkerLink isn't running, the header badge shows "DarkerLink offline". That's expected, not an error.

## Never commit

- `.env` (API keys)
- Anything generated from the game: `app/data/`, `data/icons/`, `data/wiki/`, `data/builds/`, `data/changelogs/`, `data/raw/`
- `tools/mappings/` (usmap), `tools/CUE4Parse/`, `tools/bin/`, `tools/dl/`

All of these are in `.gitignore`.

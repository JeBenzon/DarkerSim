# Darker Sim (Rust -> WebAssembly). Serves the simulator at http://127.0.0.1:8080
# Requires: rustup target add wasm32-unknown-unknown   (trunk is in tools/bin)
param([switch]$Build)
$root = Split-Path $PSScriptRoot -Parent
Push-Location (Join-Path $root "sim/crates/dad-web")
try {
  $trunk = Join-Path $root "tools/bin/trunk.exe"
  if ($Build) { & $trunk build --release } else { & $trunk serve --release --open }
} finally { Pop-Location }

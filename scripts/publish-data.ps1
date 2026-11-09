# Copies the locally generated game data into the PRIVATE data repo (EggvelopStudio/DarkerSim) and pushes it.
# Run after every game patch, after update.ps1 (or use: pwsh scripts/update.ps1 -Publish).
#
# The game data must never end up in this code repo; it is .gitignored here. This script refuses
# to publish anywhere except a checkout whose origin is EggvelopStudio/DarkerSim.
param(
    # checkout of the data repo; default ..\DarkerSim next to this repo
    [string]$DataRepo = ""
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Parent = Split-Path -Parent $Root

if (-not $DataRepo) {
    $DataRepo = Join-Path $Parent "DarkerSim"
    # the original checkout may still have its old folder name
    if (-not (Test-Path $DataRepo) -and (Test-Path (Join-Path $Parent "Dark_And_Darker\.git"))) {
        $DataRepo = Join-Path $Parent "Dark_And_Darker"
    }
}
if (-not (Test-Path (Join-Path $DataRepo ".git"))) { throw "Data repo not found: $DataRepo (pass -DataRepo <path>)" }
$DataRepo = (Resolve-Path $DataRepo).Path
if ($DataRepo -eq (Resolve-Path $Root).Path) { throw "DataRepo points at this code repo; refusing." }

# never publish game data anywhere but the private data repo
$origin = (git -C $DataRepo remote get-url origin 2>$null)
if ($origin -notmatch '(^|[:/])EggvelopStudio/DarkerSim(\.git)?/?$') {
    throw "Refusing to publish: origin of $DataRepo is '$origin', not EggvelopStudio/DarkerSim."
}

$stamp = Join-Path $Root "data\icons\_build.txt"
if (-not (Test-Path $stamp)) { throw "data\icons\_build.txt missing; run scripts/update.ps1 first." }
$Build = (Get-Content $stamp -Raw).Trim()

Write-Host "Updating data repo $DataRepo ..."
git -C $DataRepo pull --ff-only
if ($LASTEXITCODE -ne 0) { throw "git pull failed in $DataRepo" }

$folders = "app\data", "data\icons", "data\builds", "data\changelogs", "data\wiki"
$published = @()
foreach ($f in $folders) {
    $src = Join-Path $Root $f
    if (-not (Test-Path $src)) { Write-Warning "skipping $f (not generated here)"; continue }
    $dst = Join-Path $DataRepo $f
    # /MIR mirrors deletions too; exit codes below 8 mean success
    robocopy $src $dst /MIR /NFL /NDL /NJH /NJS /NP | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed for $f (exit $LASTEXITCODE)" }
    $published += ($f -replace '\\', '/')
}
$global:LASTEXITCODE = 0

git -C $DataRepo add -f -A -- $published
if ($LASTEXITCODE -ne 0) { throw "git add failed" }
git -C $DataRepo diff --cached --quiet -- $published
if ($LASTEXITCODE -eq 0) { Write-Host "No data changes for $Build; nothing to publish."; exit 0 }

git -C $DataRepo commit -m "Game data $Build" -- $published
if ($LASTEXITCODE -ne 0) { throw "git commit failed" }
git -C $DataRepo push
if ($LASTEXITCODE -ne 0) { throw "git push failed" }
Write-Host "Published game data $Build to $origin"

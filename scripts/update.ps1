# One command after every game patch:
#   pwsh scripts/update.ps1
# Extracts the installed game's data (read-only), rebuilds the app database,
# and writes a changelog against the previous build.
param(
    [string]$GameDir = "D:\SteamLibrary\steamapps\common\Dark and Darker",
    [string]$UeVersion = "UE5_6",
    [string]$Mappings = "",          # default: newest file in tools/mappings
    [switch]$Force,                  # re-extract even if this build already exists
    [switch]$SkipWiki,
    [switch]$Publish                 # afterwards copy the data to the private data repo (scripts/publish-data.ps1)
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Paks = Join-Path $GameDir "DungeonCrawler\Content\Paks"
$Build = "build-" + (Get-Content (Join-Path $GameDir "UpdateVersion.dat") -Raw).Trim()
$Raw = Join-Path $Root "data\raw\$Build"
$Exe = Join-Path $Root "tools\DadExtractor\bin\Release\net10.0\DadExtractor.exe"
if (-not $Mappings) {
    $Mappings = (Get-ChildItem (Join-Path $Root "tools\mappings\*.usmap") | Sort-Object Name | Select-Object -Last 1).FullName
}
Write-Host "Game build: $Build   mappings: $(Split-Path -Leaf $Mappings)"

if (-not (Test-Path $Exe)) {
    Write-Host "Building extractor..."
    $env:CUE4PARSE_SKIP_NATIVE = "true"
    dotnet build (Join-Path $Root "tools\DadExtractor") -c Release -v q | Out-Null
}

if ($Force -or -not (Test-Path (Join-Path $Raw "_filelist.tsv"))) {
    if (Test-Path $Raw) { Remove-Item -Recurse -Force $Raw }
    New-Item -ItemType Directory -Force $Raw | Out-Null
    & $Exe list $Paks $UeVersion (Join-Path $Raw "_filelist.tsv")
    & $Exe export $Paks $UeVersion $Mappings $Raw "DungeonCrawler/Content/DungeonCrawler/Data/"
    if ($LASTEXITCODE -ne 0) { throw "extract failed" }
} else {
    Write-Host "Raw export for $Build already exists (use -Force to redo)."
}

# Item icons (~1.4k textures -> WebP, a few seconds) and the item -> icon map. Redone per game build.
$Icons = Join-Path $Root "data\icons"
$IconStamp = Join-Path $Icons "_build.txt"
if ($Force -or -not (Test-Path $IconStamp) -or (Get-Content $IconStamp -Raw).Trim() -ne $Build) {
    Write-Host "Exporting item icons..."
    if (Test-Path $Icons) { Remove-Item -Recurse -Force $Icons }
    New-Item -ItemType Directory -Force $Icons | Out-Null
    & $Exe icons $Paks $UeVersion $Mappings $Icons 256
    & $Exe artmap $Paks $UeVersion $Mappings (Join-Path $Icons "_artmap.tsv")
    # spell / skill / music / shapeshift / perk icons for the combat picker and perk chips
    $AbilityIcons = Join-Path $Icons "ability"
    foreach ($d in "IconSpell", "IconSkill", "IconMusic", "IconShapeShift", "IconPerk") {
        & $Exe icons $Paks $UeVersion $Mappings $AbilityIcons 128 webp "UI/Resources/$d/"
    }
    # buff / debuff / shrine icons for the buff panel
    foreach ($d in "IconActorStatus", "IconReligion") {
        & $Exe icons $Paks $UeVersion $Mappings (Join-Path $Icons "buff") 128 webp "UI/Resources/$d/"
    }
    # character-skin portraits (skins like Felidian change stats; shown in the skin picker)
    & $Exe icons $Paks $UeVersion $Mappings (Join-Path $Icons "skin") 256 webp "UI/Resources/Skin/Skin_Character_"
    # class emblems for the class picker. Only icons are taken from the game:
    # all UI art (frames, panels, rules, slot glyphs) is our own, in sim/crates/dad-web/theme.css.
    & $Exe icons $Paks $UeVersion $Mappings (Join-Path $Icons "class") 256 webp "UI/Resources/IconClass/"
    if ($LASTEXITCODE -ne 0) { throw "icon export failed" }
    Set-Content $IconStamp $Build
}

if (-not $SkipWiki) { Write-Host "Refreshing wiki cache..."; python -I (Join-Path $Root "scripts\fetch_wiki.py") }

python -I (Join-Path $Root "scripts\build_db.py") $Build
if ($LASTEXITCODE -ne 0) { throw "build_db failed" }

$prev = Get-ChildItem (Join-Path $Root "data\builds\build-*.json") |
    Where-Object { $_.BaseName -ne $Build } |
    Sort-Object { [int]($_.BaseName -replace '\D', '') } | Select-Object -Last 1
if ($prev) {
    python -I (Join-Path $Root "scripts\diff_builds.py") $prev.BaseName $Build
} else {
    Write-Host "No previous build to diff against yet (this is the baseline)."
}
if ($Publish) { & (Join-Path $PSScriptRoot "publish-data.ps1") }
Write-Host "Done. Open app\index.html"

# Nainstaluje plugin Terén do Open CAD Studia (verze podle vydání – viz README).
# Spouští se přes nainstalovat.bat (dvojklik).
#
# Je-li vedle skriptu hotový plugin (teren_plugin.dll), jen se zkopíruje.
# Jinak se sestaví ze zdrojů – k tomu je potřeba Rust (https://rustup.rs);
# na verzi Rustu nezáleží.

$ErrorActionPreference = "Stop"
Set-Location -Path $PSScriptRoot

function Konec($kod) {
    Write-Host ""
    Read-Host "Stiskněte Enter pro zavření okna"
    exit $kod
}

if ($PSScriptRoot -like "$env:TEMP*") {
    Write-Host "Zdá se, že instalace běží přímo ze zipu. Zip nejdřív rozbalte (pravým tlačítkem › Extrahovat vše) a spusťte nainstalovat.bat z rozbalené složky." -ForegroundColor Yellow
    Konec 1
}

$dll = Join-Path $PSScriptRoot "teren_plugin.dll"
if (-not (Test-Path $dll)) {
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargo) {
        $cesta = Join-Path $env:USERPROFILE ".cargo\bin"
        if (Test-Path (Join-Path $cesta "cargo.exe")) {
            $env:PATH = "$cesta;$env:PATH"
        } else {
            Write-Host "CHYBA: hotový plugin (teren_plugin.dll) tu není a Rust (cargo) pro sestavení také ne." -ForegroundColor Red
            Write-Host "Stáhněte zip s hotovým pluginem, nebo nainstalujte Rust z https://rustup.rs" -ForegroundColor Red
            Konec 1
        }
    }
    Write-Host "Sestavení pluginu (poprvé to trvá několik minut)..." -ForegroundColor Cyan
    # bez samoaktualizace rustupu – ta na Windows umí zamknout soubory a shodit sestavení
    $env:RUSTUP_AUTO_SELF_UPDATE = "disable"
    cargo build --release --locked -p teren_plugin
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Zkouším sestavení znovu..." -ForegroundColor Yellow
        Start-Sleep -Seconds 5
        cargo build --release --locked -p teren_plugin
    }
    if ($LASTEXITCODE -ne 0) {
        Write-Host "CHYBA: sestavení se nepovedlo (viz výpis výše)." -ForegroundColor Red
        Konec 1
    }
    $dll = "target\release\teren_plugin.dll"
}

Write-Host "Instalace do Open CAD Studia..." -ForegroundColor Cyan
$cil = Join-Path $env:APPDATA "OpenCADStudio\plugins\cz.fluvio.teren"
New-Item -ItemType Directory -Force -Path $cil | Out-Null
Get-ChildItem $cil -Filter *.dll -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue

try {
    Copy-Item $dll (Join-Path $cil "teren_plugin.dll") -Force
} catch {
    Write-Host "CHYBA: nelze přepsat plugin – zavřete Open CAD Studio a spusťte instalaci znovu." -ForegroundColor Red
    Konec 1
}
$toml = Join-Path $PSScriptRoot "plugin.toml"
if (-not (Test-Path $toml)) { $toml = "plugin\plugin.toml" }
Copy-Item $toml (Join-Path $cil "plugin.toml") -Force

Write-Host ""
Write-Host "Hotovo. Plugin je nainstalovaný v:" -ForegroundColor Green
Write-Host "  $cil"
Write-Host ""
Write-Host "Spusťte (nebo restartujte) Open CAD Studio – objeví se záložka Terén."
Konec 0

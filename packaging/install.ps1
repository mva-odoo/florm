# Installe ou met à jour Florm sous Windows, sans l'avertissement SmartScreen et sans droits admin :
#   irm https://raw.githubusercontent.com/mva-odoo/florm/main/packaging/install.ps1 | iex
# SmartScreen ne vérifie que les fichiers marqués « téléchargés d'Internet » par le navigateur ;
# Invoke-WebRequest ne les marque pas. Le .exe est vérifié avec le SHA256SUMS de la release.
# Installé dans %LOCALAPPDATA%\Programs\Florm, avec un raccourci dans le menu Démarrer, la commande
# `florm` dans le PATH et une entrée dans Paramètres → Applications pour le désinstaller.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'  # la barre de progression rend Invoke-WebRequest très lent
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = 'mva-odoo/florm'
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
$version = $release.tag_name.TrimStart('v')
$name = "florm-$version-windows-x86_64.exe"
$asset = $release.assets | Where-Object name -eq $name
$sumsAsset = $release.assets | Where-Object name -eq 'SHA256SUMS'
if (-not $asset -or -not $sumsAsset) { throw "Pas de $name ou de SHA256SUMS dans la release $version" }

$dir = Join-Path $env:LOCALAPPDATA 'Programs\Florm'
$exe = Join-Path $dir 'florm.exe'
New-Item -ItemType Directory -Force $dir | Out-Null
$new = "$exe.download"
$sums = Join-Path $dir 'SHA256SUMS.download'
Write-Host "Téléchargement de Florm $version…"
Invoke-WebRequest $asset.browser_download_url -OutFile $new -UseBasicParsing
Invoke-WebRequest $sumsAsset.browser_download_url -OutFile $sums -UseBasicParsing
$line = Get-Content $sums | Where-Object { ($_ -split '\s+')[1] -in $name, "*$name" } | Select-Object -First 1
Remove-Item $sums
$expected = if ($line) { ($line -split '\s+')[0] } else { '' }
if ((Get-FileHash $new -Algorithm SHA256).Hash -ne $expected) {
    Remove-Item $new
    throw "$name est corrompu (somme SHA-256 différente de SHA256SUMS)"
}

# un .exe en cours d'exécution ne peut pas être écrasé mais peut être renommé (Florm supprime le
# .old à son prochain démarrage, comme après une mise à jour intégrée)
if (Test-Path $exe) { Move-Item $exe "$exe.old" -Force }
Move-Item $new $exe
Unblock-File $exe

$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Florm.lnk'
$link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
$link.TargetPath = $exe
$link.Save()

# PATH lu et écrit tel quel dans le registre : [Environment]::SetEnvironmentVariable l'écrirait en
# REG_SZ, ce qui casse les entrées de l'utilisateur en %VARIABLE%
function Set-UserPath {
    param([scriptblock]$change)
    $old = (Get-Item 'HKCU:\Environment').GetValue('Path', '', 'DoNotExpandEnvironmentNames')
    $new = (& $change @(($old -split ';') -ne '')) -join ';'
    if ($new -ne $old) {
        Set-ItemProperty 'HKCU:\Environment' Path $new -Type ExpandString
        # prévient l'Explorateur (nouveaux terminaux) que l'environnement a changé
        [Environment]::SetEnvironmentVariable('FLORM_INSTALL', '1', 'User')
        [Environment]::SetEnvironmentVariable('FLORM_INSTALL', $null, 'User')
    }
}
Set-UserPath { param($p) if ($p -contains $dir) { $p } else { $p + $dir } }

$uninstall = Join-Path $dir 'uninstall.ps1'
"function Set-UserPath {${function:Set-UserPath}}" + @'

# Désinstalle Florm (les préférences de %APPDATA%\florm sont gardées).
$dir = $PSScriptRoot
Get-Process florm -ErrorAction SilentlyContinue | Where-Object Path -eq (Join-Path $dir 'florm.exe') | Stop-Process -Force
Remove-Item (Join-Path ([Environment]::GetFolderPath('Programs')) 'Florm.lnk') -ErrorAction SilentlyContinue
Set-UserPath { param($p) $p -ne $dir }
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Florm' -Recurse -ErrorAction SilentlyContinue
Remove-Item $dir -Recurse -Force
'@ | Set-Content $uninstall -Encoding UTF8

$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Florm'
New-Item $key -Force | Out-Null
Set-ItemProperty $key DisplayName 'Florm'
Set-ItemProperty $key Publisher 'Florm'
Set-ItemProperty $key DisplayIcon $exe
Set-ItemProperty $key InstallLocation $dir
Set-ItemProperty $key UninstallString "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$uninstall`""
Set-ItemProperty $key NoModify 1 -Type DWord
Set-ItemProperty $key NoRepair 1 -Type DWord

Write-Host "Florm $version est installé : menu Démarrer → Florm, ou « florm » dans un nouveau terminal."

# 本機打包 Windows 安裝檔。
#
# CI 的 release.yml 才是正式出版路徑；這一支是給「想在自己機器上先確認一次」用的。
#
# 用法：
#   .\build-installer.ps1              # 用現在的版號打包
#   .\build-installer.ps1 -Version 1.0.0   # 同步三個檔的版號再打包
#
# ★ 版號在三個檔案裡各有一份，而它們必須一致：
#   package.json（前端顯示的版本）、src-tauri/Cargo.toml（binary 的版本）、
#   src-tauri/tauri.conf.json（安裝檔與更新檢查看的版本）。
#   對不上的症狀是「關於」顯示 0.9.0、安裝檔卻叫 1.0.0，而使用者回報時
#   會報錯的那一個。

param(
  [string]$Version = ''
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot

function Set-Version([string]$v) {
  Write-Host "同步版號 -> $v"

  $pkgPath = Join-Path $root 'package.json'
  $pkg = Get-Content $pkgPath -Raw -Encoding UTF8
  $pkg = [regex]::Replace($pkg, '"version":\s*"[^"]*"', """version"": ""$v""", 1)
  [System.IO.File]::WriteAllText($pkgPath, $pkg, (New-Object System.Text.UTF8Encoding $false))

  $confPath = Join-Path $root 'src-tauri/tauri.conf.json'
  $conf = Get-Content $confPath -Raw -Encoding UTF8
  $conf = [regex]::Replace($conf, '"version":\s*"[^"]*"', """version"": ""$v""", 1)
  [System.IO.File]::WriteAllText($confPath, $conf, (New-Object System.Text.UTF8Encoding $false))

  $cargoPath = Join-Path $root 'src-tauri/Cargo.toml'
  $cargo = Get-Content $cargoPath -Raw -Encoding UTF8
  $cargo = [regex]::Replace($cargo, '(?m)^version\s*=\s*"[^"]*"', "version = ""$v""", 1)
  [System.IO.File]::WriteAllText($cargoPath, $cargo, (New-Object System.Text.UTF8Encoding $false))
}

function Assert-VersionsMatch {
  $pkg = (Get-Content (Join-Path $root 'package.json') -Raw -Encoding UTF8 |
    Select-String -Pattern '"version":\s*"([^"]*)"').Matches[0].Groups[1].Value
  $conf = (Get-Content (Join-Path $root 'src-tauri/tauri.conf.json') -Raw -Encoding UTF8 |
    Select-String -Pattern '"version":\s*"([^"]*)"').Matches[0].Groups[1].Value
  $cargo = (Get-Content (Join-Path $root 'src-tauri/Cargo.toml') -Raw -Encoding UTF8 |
    Select-String -Pattern '(?m)^version\s*=\s*"([^"]*)"').Matches[0].Groups[1].Value

  if (($pkg -ne $conf) -or ($pkg -ne $cargo)) {
    throw "版號對不上：package.json=$pkg tauri.conf.json=$conf Cargo.toml=$cargo`n用 -Version 參數同步它們。"
  }
  Write-Host "版號 $pkg（三個檔一致）"
}

if ($Version -ne '') { Set-Version $Version }
Assert-VersionsMatch

Push-Location $root
try {
  Write-Host '--- 前端測試與建置 ---'
  npm ci
  if ($LASTEXITCODE -ne 0) { throw 'npm ci 失敗' }
  npm test
  if ($LASTEXITCODE -ne 0) { throw '前端測試沒過 —— 不要打包一個測試紅的版本' }

  Write-Host '--- Rust 測試 ---'
  Push-Location (Join-Path $root 'src-tauri')
  try {
    cargo test --no-default-features --features server
    if ($LASTEXITCODE -ne 0) { throw 'Rust 測試沒過' }
    cargo clippy --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'clippy 沒過' }
  } finally { Pop-Location }

  Write-Host '--- 打包 ---'
  npm run tauri build
  if ($LASTEXITCODE -ne 0) { throw 'tauri build 失敗' }

  $out = Join-Path $root 'src-tauri/target/release/bundle'
  Write-Host ''
  Write-Host "安裝檔在 $out"
  Get-ChildItem $out -Recurse -Include *.exe, *.msi | ForEach-Object {
    Write-Host ("  {0}  ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB))
  }
} finally {
  Pop-Location
}

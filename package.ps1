# Builds a release and lays out a folder ready to zip and hand to someone.
#
#   .\package.ps1                 -> dist\Primitive-1.0.0
#   .\package.ps1 -Zip            -> also dist\Primitive-1.0.0.zip
#
# What ends up in there is deliberate. The two binaries, the assets they
# need, and the plugins folder -- and *no* settings files. Both binaries
# write their own on first run with every value present and commented in
# the README, which is a better starting point than whatever happened to
# be in the build directory. Shipping a config from a developer's machine
# is how a release ends up pointing at 127.0.0.1 forever.

param(
    [switch]$Zip,
    [string]$OutDir = "dist"
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot

$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version\s*=\s*"(.+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$name = "Primitive-$version"
$target = Join-Path $root (Join-Path $OutDir $name)

Write-Host "building $name (release)" -ForegroundColor Cyan
Push-Location $root
try {
    # `$ErrorActionPreference = "Stop"` turns anything a native command
    # writes to stderr into a terminating error -- and cargo writes its
    # entire progress log there, so a perfectly successful build would
    # abort this script. The exit code is the only honest signal, so
    # relax the preference around the call and check that instead.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    cargo build --release --workspace
    $code = $LASTEXITCODE
    $ErrorActionPreference = $previous
    if ($code -ne 0) { throw "cargo build failed (exit $code)" }
} finally {
    Pop-Location
}

if (Test-Path $target) { Remove-Item -Recurse -Force $target }
New-Item -ItemType Directory -Force -Path $target | Out-Null

$release = Join-Path $root "target\release"
foreach ($exe in @("primitive_client.exe", "primitive_server.exe")) {
    Copy-Item (Join-Path $release $exe) $target
}

# Assets sit next to the executable; `resolve_assets_dir` looks there
# first, so a packaged build finds them without any configuration.
Copy-Item -Recurse (Join-Path $root "assets") (Join-Path $target "assets")
Copy-Item -Recurse (Join-Path $root "plugins") (Join-Path $target "plugins")
foreach ($doc in @("README.md", "CHANGELOG.md", "LICENSE")) {
    Copy-Item (Join-Path $root $doc) $target
}

$size = "{0:N1}" -f ((Get-ChildItem -Recurse $target | Measure-Object -Property Length -Sum).Sum / 1MB)
Write-Host "packaged $target ($size MB)" -ForegroundColor Green

if ($Zip) {
    $archive = "$target.zip"
    if (Test-Path $archive) { Remove-Item -Force $archive }
    Compress-Archive -Path $target -DestinationPath $archive
    Write-Host "wrote $archive" -ForegroundColor Green
}

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

    # Two invocations, deliberately, not one `--workspace`.
    #
    # Cargo unifies features across a single build. The server binary
    # wants its `plugins` feature; the client asks for the server with
    # `default-features = false` precisely so the scripting engine is not
    # in the game. Build both at once and the union wins: the client
    # links a plugins-enabled server and ships rhai after all -- about
    # two megabytes of scripting engine, and a guarantee in the docs that
    # quietly stopped being true. Separate invocations resolve features
    # separately.
    cargo build --release -p primitive_client
    $clientCode = $LASTEXITCODE
    cargo build --release -p primitive_server
    $serverCode = $LASTEXITCODE

    $ErrorActionPreference = $previous
    if ($clientCode -ne 0) { throw "building the client failed (exit $clientCode)" }
    if ($serverCode -ne 0) { throw "building the server failed (exit $serverCode)" }
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
# GUIDE first: it is the one a player actually needs. README is the
# design log and CHANGELOG is history.
#
# An ASCII filename on purpose. PowerShell 5.1 reads this script as the
# system codepage, so a Cyrillic literal here arrives mangled -- and a
# non-ASCII name inside a zip is a portability hazard besides. The
# document itself is still in Russian.
foreach ($doc in @("GUIDE.md", "README.md", "CHANGELOG.md", "LICENSE")) {
    Copy-Item (Join-Path $root $doc) $target
}

$size = "{0:N1}" -f ((Get-ChildItem -Recurse $target | Measure-Object -Property Length -Sum).Sum / 1MB)
Write-Host "packaged $target ($size MB)" -ForegroundColor Green

if ($Zip) {
    # Named by platform rather than by version: this is the file people
    # link to and download, and a name that changes every release breaks
    # every link to it. The version is inside, in the folder name and in
    # the game's own menu.
    $archive = Join-Path (Split-Path $target) "primitive_win64.zip"
    if (Test-Path $archive) { Remove-Item -Force $archive }
    # The folder goes in with it, so extracting does not scatter eight
    # files into whatever directory the user happened to be in.
    Compress-Archive -Path $target -DestinationPath $archive
    $mb = "{0:N1}" -f ((Get-Item $archive).Length / 1MB)
    Write-Host "wrote $archive ($mb MB)" -ForegroundColor Green
}

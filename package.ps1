# Builds a release and lays out a folder ready to zip and hand to someone.
#
#   .\package.ps1                 -> dist\Primitive-1.0.0
#   .\package.ps1 -Zip            -> also dist\Primitive-1.0.0.zip
#
# What ends up in there is deliberate. The two binaries (the client
# carries its own assets), the player's guide, and the plugins folder -- and *no* settings files. Both binaries
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

# **No `assets` folder.** Every file in it -- 424 pictures, 412
# recordings, 26 models, the font and `blocks.toml` -- is compiled into
# `primitive_client.exe` (`embedded.rs`, and tests there, in
# `audio::recorded` and in `logic::models` fail when one is not). A copy
# beside the executable was the same 4 MB a second time: 32 MB of
# unpacked release where 28 would do, for nothing the game reads
# differently. The folder is still how a resource pack works -- a file
# at `assets/textures/stone.png` beside the executable wins over the
# built-in one -- so a player who wants one makes the folder and puts
# in only what they replace, which is also the only way a pack stays
# readable when the next release changes the other four hundred files.
#
# Rejected: shipping the folder as a template to edit. A full copy of
# the defaults on disk *overrides* the defaults, so an old folder
# carried into a new release silently puts back last version's
# textures -- the resource-pack rule turns a convenience into a bug.
Copy-Item -Recurse (Join-Path $root "plugins") (Join-Path $target "plugins")
# GUIDE only, of the three documents: it is the one a player needs.
# README is the design log (250 KB) and CHANGELOG is the history
# (2.4 MB, and growing with every release) -- both are for whoever
# works on the game, both live in the repository, and together they
# were a tenth of the release folder. LICENSE goes because it has to.
#
# An ASCII filename on purpose. PowerShell 5.1 reads this script as the
# system codepage, so a Cyrillic literal here arrives mangled -- and a
# non-ASCII name inside a zip is a portability hazard besides. The
# document itself is still in Russian.
foreach ($doc in @("GUIDE.md", "LICENSE", "LICENSE-APACHE", "LICENSE-ASSETS", "NOTICE")) {
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

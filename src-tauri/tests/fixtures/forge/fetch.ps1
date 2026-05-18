# Downloads Forge installer fixtures pinned in SHA1SUMS.
#
# Usage from repo root:
#   pwsh src-tauri/tests/fixtures/forge/fetch.ps1
# or from the script's directory:
#   pwsh fetch.ps1
#
# After download, each file's sha1 is verified. Mismatch = abort.

$ErrorActionPreference = "Stop"
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$installersDir = Join-Path $scriptDir "installers"
$sha1File = Join-Path $scriptDir "SHA1SUMS"

if (-not (Test-Path $installersDir)) {
    New-Item -ItemType Directory -Force -Path $installersDir | Out-Null
}

$lines = Get-Content $sha1File | Where-Object { -not ($_ -match "^\s*#" -or $_ -match "^\s*$") }

foreach ($line in $lines) {
    $parts = $line -split "\s+", 2
    if ($parts.Length -ne 2) { continue }
    $expectedSha = $parts[0].ToLower()
    $name = $parts[1].Trim()
    $dest = Join-Path $installersDir $name

    if (Test-Path $dest) {
        $actualSha = (Get-FileHash -Algorithm SHA1 $dest).Hash.ToLower()
        if ($actualSha -eq $expectedSha) {
            Write-Output "ok: $name"
            continue
        } else {
            Write-Warning "sha1 mismatch on existing $name (expected $expectedSha, got $actualSha) - re-downloading"
            Remove-Item $dest
        }
    }

    # Derive download URL from filename pattern.
    #   Forge:    forge-<mc>-<fv>-installer.jar
    #   NeoForge: neoforge-<nv>-installer.jar  (no <mc> in filename — encoded by major.minor pair)
    if ($name -match "^forge-([^-]+(?:-[^-]+)?)-([0-9.]+)-installer\.jar$") {
        $flavor = "forge"
        $mc = $matches[1]
        $fv = $matches[2]
    } elseif ($name -match "^neoforge-([0-9.]+(?:-beta|-rc[0-9]+)?)-installer\.jar$") {
        $flavor = "neoforge"
        $fv = $matches[1]
        $mc = $null
    } else {
        throw "Cannot derive URL for unrecognized filename: $name"
    }

    if ($flavor -eq "forge") {
        # Some MC ranges (1.7.10, parts of 1.9) use the legacy
        # `<mc>-<fv>-<mc>` quirk in maven paths. The Rust meta layer
        # detects this at runtime via maven-metadata.xml; here we use a
        # static allowlist of known affected MCs to keep the script
        # offline-deterministic. If you add a new fixture and hit a 404,
        # extend this set.
        $quirkMcs = @("1.7.10")
        $rawSegment = "${mc}-${fv}"
        $rawName = $name
        if ($quirkMcs -contains $mc) {
            $rawSegment = "${mc}-${fv}-${mc}"
            $rawName = "forge-${rawSegment}-installer.jar"
        }
        $url = "https://maven.minecraftforge.net/net/minecraftforge/forge/${rawSegment}/${rawName}"
    } else {
        # NeoForge: maven.neoforged.net/releases/.../neoforge/<nv>/neoforge-<nv>-installer.jar.
        $url = "https://maven.neoforged.net/releases/net/neoforged/neoforge/${fv}/neoforge-${fv}-installer.jar"
    }

    Write-Output "downloading: $url"
    Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing

    $actualSha = (Get-FileHash -Algorithm SHA1 $dest).Hash.ToLower()
    if ($actualSha -ne $expectedSha) {
        Remove-Item $dest
        throw "sha1 mismatch on $name (expected $expectedSha, got $actualSha)"
    }
    Write-Output "ok: $name"
}

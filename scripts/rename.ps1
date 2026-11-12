<#
.SYNOPSIS
  Turn this template into a named project.

.DESCRIPTION
  Replaces the template's placeholder names in every tracked text file:

    LocalApp   -> the display name           (e.g. "Recipe Box")
    LOCALAPP   -> the environment prefix     (e.g. RECIPE_BOX)
    localapp_  -> the Rust identifier prefix (e.g. recipe_box_)
    localapp   -> the crate/image/URL name   (e.g. recipe-box)

  Run it once, from the repository root, right after creating the project.

.EXAMPLE
  ./scripts/rename.ps1 -Name recipe-box -DisplayName "Recipe Box"
#>
param(
    [Parameter(Mandatory = $true)][string]$Name,
    [Parameter(Mandatory = $true)][string]$DisplayName
)

$ErrorActionPreference = 'Stop'

if ($Name -cnotmatch '^[a-z][a-z0-9]*(-[a-z0-9]+)*$') {
    throw "Name must be lowercase kebab-case, e.g. recipe-box (got '$Name')."
}
$snake = $Name -replace '-', '_'
$upper = $snake.ToUpperInvariant()

Set-Location (Join-Path $PSScriptRoot '..')
$files = git ls-files --cached --others --exclude-standard |
    Where-Object { $_ -notmatch '^scripts/rename\.(ps1|sh)$' -and $_ -notmatch '\.(png|jpg|jpeg|gif|ico|pdf|woff2?|sqlite)$' }

$utf8 = New-Object System.Text.UTF8Encoding $false
$changed = 0
foreach ($file in $files) {
    if (-not (Test-Path $file -PathType Leaf)) { continue }
    $path = (Resolve-Path $file).Path
    $text = [System.IO.File]::ReadAllText($path)
    $new = $text.Replace('LocalApp', $DisplayName).Replace('LOCALAPP', $upper).Replace('localapp_', "${snake}_").Replace('localapp', $Name)
    if ($new -cne $text) {
        [System.IO.File]::WriteAllText($path, $new, $utf8)
        $changed++
        Write-Host "updated $file"
    }
}

Write-Host ""
Write-Host "Renamed to $DisplayName ($Name) in $changed files."
Write-Host "Next: cargo test --workspace, then edit README.md, the authors in Cargo.toml and LICENSE."

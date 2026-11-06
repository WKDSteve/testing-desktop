<#
.SYNOPSIS
Try a release of testing-desktop on this machine in one command: download, check, install, start, repair.

.DESCRIPTION
The trial a person owes a release (docs/RELEASING.md, step 5), as one command instead of six. It takes the
files of the release (the archive, install.ps1, uninstall.ps1, SHA256SUMS) into a temporary folder and then, one
line per step:

  1. checks every file against SHA256SUMS,
  2. installs the archive with the install.ps1 of the release (-File, nothing else is downloaded),
  3. asks the installed program for its version (it must be the one of the tag),
  4. starts the hub, asks it for a pairing link and its first page, stops it,
  5. installs again (the repair) and asks for the version again,
  6. with -Uninstall: removes it with the uninstall.ps1 of the release and checks that the program is gone.

It works on the real installation of the current user: a hub that runs is stopped, an installed program is
replaced by the one of the release. Without -Uninstall the release stays installed. Your settings are never
removed unless you pass -PurgeSettings with -Uninstall. Your repositories and results are not touched.

The files come through `gh release download` when gh is there (the only way for a draft), else by plain
download (a published release). Run it in your own terminal, not elevated and not from an agent session: an
app that redirects the profile would install into its own copy.

.PARAMETER Tag
The tag of the release, for example v0.1.0. Default: the latest published release.

.PARAMETER Repo
The repository, owner/name.

.PARAMETER Uninstall
Also try the uninstall script at the end. The program is then gone.

.PARAMETER PurgeSettings
With -Uninstall: also remove the settings of the app (config.json and the files of the hub).

.PARAMETER AllowElevated
Handed to the scripts of the release, for a build machine that runs elevated.

.EXAMPLE
.\scripts\try-release.ps1 -Tag v0.1.0

.EXAMPLE
.\scripts\try-release.ps1 -Tag v0.1.0 -Uninstall
#>
[CmdletBinding()]
param(
    [string] $Tag = '',
    [string] $Repo = 'WKDSteve/testing-desktop',
    [switch] $Uninstall,
    [switch] $PurgeSettings,
    [switch] $AllowElevated
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Exit-Trial([string] $Message) {
    throw "try-release.ps1: $Message"
}

if ($PurgeSettings -and -not $Uninstall) { Exit-Trial '-PurgeSettings belongs to -Uninstall' }
if ($Repo -notmatch '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$') { Exit-Trial "not a repository (owner/name): $Repo" }

$script:failures = 0
function Write-Check([bool] $Passed, [string] $Name) {
    if ($Passed) { Write-Output "ok   $Name" }
    else { Write-Output "FAIL $Name"; $script:failures++ }
}

# A native program: its output as text and its exit code, without a red error record for a line on stderr.
function Invoke-Program([string] $File, [string[]] $Arguments) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $text = (& $File @Arguments 2>&1 | Out-String).TrimEnd()
        return [pscustomobject]@{ Code = $LASTEXITCODE; Text = $text }
    }
    finally {
        $ErrorActionPreference = $previous
    }
}

# A script of the release in a PowerShell of its own: an `exit` or a throw in it must not end this run.
function Invoke-ReleaseScript([string] $File, [string[]] $Arguments) {
    $shell = (Get-Process -Id $PID).Path
    return Invoke-Program $shell (@('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $File) + $Arguments)
}

$gh = Get-Command gh -ErrorAction SilentlyContinue
if ($Tag -eq '') {
    if ($gh) {
        $asked = Invoke-Program $gh.Source @('release', 'view', '--repo', $Repo, '--json', 'tagName', '--jq', '.tagName')
        if ($asked.Code -ne 0) { Exit-Trial "cannot ask for the latest release of ${Repo}: $($asked.Text)" }
        $Tag = $asked.Text.Trim()
    }
    else {
        $Tag = (Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ 'User-Agent' = 'try-release' }).tag_name
    }
}
if ($Tag -notmatch '^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$') { Exit-Trial "not a release tag (vX.Y.Z): $Tag" }
$version = $Matches[1]
$archiveName = "testing-hub-$version-x86_64-pc-windows-msvc.zip"
$names = @($archiveName, 'install.ps1', 'uninstall.ps1', 'SHA256SUMS')

$installDir = $env:TESTING_DESKTOP_INSTALL_DIR
if (-not $installDir) { $installDir = Join-Path $env:LOCALAPPDATA 'testing-desktop\bin' }
$program = Join-Path $installDir 'testing-hub.exe'

$tmp = Join-Path ([IO.Path]::GetTempPath()) ('testing-desktop-try-release-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
$started = $false
try {
    Write-Output "trying $Repo $Tag (the files are in $tmp)"

    # The files of the release.
    if ($gh) {
        $got = Invoke-Program $gh.Source @('release', 'download', $Tag, '--repo', $Repo, '--dir', $tmp)
        if ($got.Code -ne 0) { Exit-Trial "gh release download failed: $($got.Text)" }
    }
    else {
        foreach ($name in $names) {
            Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/$Repo/releases/download/$Tag/$name" -OutFile (Join-Path $tmp $name)
        }
    }
    $missing = @($names | Where-Object { -not (Test-Path -LiteralPath (Join-Path $tmp $_) -PathType Leaf) })
    Write-Check ($missing.Count -eq 0) "the release has its files ($($names -join ', '))"
    if ($missing.Count -gt 0) { Exit-Trial "missing in the release: $($missing -join ', ')" }

    # 1. The sums: every file the list names, and the archive must be among them.
    $listed = @{}
    foreach ($line in Get-Content -LiteralPath (Join-Path $tmp 'SHA256SUMS')) {
        if ($line -match '^([0-9a-fA-F]{64})\s+\*?(\S.*)$') { $listed[$Matches[2].Trim()] = $Matches[1].ToLowerInvariant() }
    }
    $wrong = @()
    foreach ($name in $listed.Keys) {
        $file = Join-Path $tmp $name
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { $wrong += "$name (not there)"; continue }
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $listed[$name]) { $wrong += $name }
    }
    $sumsOk = $listed.ContainsKey($archiveName) -and $listed.ContainsKey('install.ps1') -and $wrong.Count -eq 0
    Write-Check $sumsOk "every file matches SHA256SUMS$(if ($wrong.Count -gt 0) { ': not ' + ($wrong -join ', ') })"
    if (-not $sumsOk) { Exit-Trial 'the sums do not match: nothing is installed' }

    $elevated = @()
    if ($AllowElevated) { $elevated = @('-AllowElevated') }
    $installArgs = @('-File', (Join-Path $tmp $archiveName)) + $elevated

    # 2. and 3. Install, and the program says the version of the tag.
    $done = Invoke-ReleaseScript (Join-Path $tmp 'install.ps1') $installArgs
    Write-Check ($done.Code -eq 0) 'install.ps1 installs the archive'
    if ($done.Code -ne 0) { Write-Output $done.Text; Exit-Trial 'the install failed (see above)' }
    $said = Invoke-Program $program @('--version')
    Write-Check ($said.Code -eq 0 -and $said.Text -match ('^testing-hub ' + [regex]::Escape($version) + '(\s|$)')) "the installed program says $version (it says: $($said.Text))"

    # 4. The hub starts, answers, and stops.
    $run = Invoke-Program $program @('start')
    $started = $run.Code -eq 0
    Write-Check $started "the hub starts ($($run.Text))"
    if ($started) {
        $link = Invoke-Program $program @('open')
        $address = ($link.Text -split "`r?`n")[0]
        $linkOk = $link.Code -eq 0 -and $address -match '^(http://127\.0\.0\.1:\d+)/#pair=[0-9a-f]+$'
        Write-Check $linkOk 'the hub gives a pairing link'
        if ($linkOk) {
            $page = $null
            try { $page = Invoke-WebRequest -UseBasicParsing -Uri ($Matches[1] + '/') -TimeoutSec 15 } catch { $page = $null }
            Write-Check ($null -ne $page -and $page.StatusCode -eq 200) 'the hub serves its page'
        }
        $stop = Invoke-Program $program @('stop')
        $started = $stop.Code -ne 0
        Write-Check ($stop.Code -eq 0) "the hub stops ($($stop.Text))"
    }

    # 5. The repair: the same install again.
    $again = Invoke-ReleaseScript (Join-Path $tmp 'install.ps1') $installArgs
    $said = Invoke-Program $program @('--version')
    Write-Check ($again.Code -eq 0 -and $said.Text -match ('^testing-hub ' + [regex]::Escape($version) + '(\s|$)')) 'a second install repairs it'
    if ($again.Code -ne 0) { Write-Output $again.Text }

    # 6. The way out, when asked for.
    if ($Uninstall) {
        $flags = $elevated
        if ($PurgeSettings) { $flags = @('-PurgeSettings') + $flags }
        $gone = Invoke-ReleaseScript (Join-Path $tmp 'uninstall.ps1') $flags
        Write-Check ($gone.Code -eq 0 -and -not (Test-Path -LiteralPath $program)) "uninstall.ps1 removes the program$(if ($PurgeSettings) { ' and the settings' })"
        if ($gone.Code -ne 0) { Write-Output $gone.Text }
        $twice = Invoke-ReleaseScript (Join-Path $tmp 'uninstall.ps1') $flags
        Write-Check ($twice.Code -eq 0) 'a second uninstall finds nothing to do and says so'
    }
}
finally {
    # A hub this run started and could not stop is not left running behind a failed trial.
    if ($started -and (Test-Path -LiteralPath $program)) { $null = Invoke-Program $program @('stop') }
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output ''
if ($script:failures -gt 0) {
    Write-Output "$($script:failures) step(s) failed: do not publish $Tag"
    exit 1
}
if ($Uninstall) { Write-Output "$Tag passed the trial; it is removed again" }
else { Write-Output "$Tag passed the trial and is installed: $program" }

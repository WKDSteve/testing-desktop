<#
.SYNOPSIS
Build, sign and (with -Upload) publish the Android app of testing-desktop as a development build.

.DESCRIPTION
The development APK is built and signed on the private machine only (MREL-06), with the development key of this
machine (MREL-01 comes for the first tagged release, MREL-02). No key and no secret exists in GitHub: the keystore is
a file outside every repository, its path comes from TESTING_ANDROID_DEV_KEYSTORE (or -Keystore), its password is
asked for when the APK is signed and never written anywhere.

The steps, one line each:

  1. refuses on a workstation or on a machine that is not known to be private, and without a keystore,
  2. takes the commit (default: the tip of origin/main) and requires its CI to be green (GIT-08),
  3. builds the page and the release APK (aarch64) in a temporary worktree of that commit,
  4. aligns and signs it (zipalign, apksigner) and checks the signature and, if given, the certificate,
  5. writes android.json (commit, sizes, checksums) and SHA256SUMS next to it,
  6. with -Upload: puts the three files on the pre-release `android-dev` of the repository (made if missing, never
     the latest release), and removes the older APKs there.

The files do not go to the rolling release `latest`: its publish step removes every asset that is not part of the
build it publishes. Without -Upload nothing leaves the machine; the commands that would run are printed.

.PARAMETER Sha
The commit to build. Default: the tip of origin/main. With -Apk it only names the result.

.PARAMETER Keystore
The keystore file. Default: $env:TESTING_ANDROID_DEV_KEYSTORE.

.PARAMETER Alias
The key alias in the keystore.

.PARAMETER Apk
An unsigned APK to sign instead of building one (reruns, the test).

.PARAMETER KeyEnvVar
The name of an environment variable that holds the keystore password, for the test with a throwaway key only.
Without it the password is asked for.

.PARAMETER CertSha256
The SHA-256 of the certificate the APK must carry (default: $env:TESTING_ANDROID_DEV_CERT_SHA256). A different
certificate fails: the wrong keystore must not sign an APK that replaces the one on a phone.

.PARAMETER Upload
Publish. Without it the signed APK stays in -OutDir.

.EXAMPLE
pwsh scripts/android-sign-and-upload.ps1            # build and sign the tip of main, publish nothing
pwsh scripts/android-sign-and-upload.ps1 -Upload    # and put it on the pre-release android-dev
#>
[CmdletBinding()]
param(
  [string]$Repo = 'WKDSteve/testing-desktop',
  [string]$Sha,
  [string]$Keystore = $env:TESTING_ANDROID_DEV_KEYSTORE,
  [string]$Alias = 'testing-dev',
  [string]$Apk,
  [string]$OutDir = (Join-Path ([IO.Path]::GetTempPath()) 'testing-android-dev'),
  [string]$Tag = 'android-dev',
  [string]$KeyEnvVar,
  [string]$CertSha256 = $env:TESTING_ANDROID_DEV_CERT_SHA256,
  [switch]$Upload
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Fail([string]$Message, [int]$Code = 2) {
  [Console]::Error.WriteLine("android-sign-and-upload: $Message")
  exit $Code
}
function Say([string]$Message) { [Console]::Out.WriteLine("android-sign-and-upload: $Message") }
function Invoke-Native {
  # A native command whose failure stops the script (the exit code is not an error to PowerShell).
  $name = $args[0]
  # A variable is splatted into separate arguments; `@(...)` of an expression is one array argument that a native
  # command receives as one string ("install --frozen-lockfile" as the name of a pnpm command).
  $rest = @($args | Select-Object -Skip 1)
  & $name @rest
  if ($LASTEXITCODE -ne 0) { Fail "'$name' failed with exit code $LASTEXITCODE" 1 }
}

# 1. The machine and the key come first: nothing slow runs on a machine that must not sign.
$role = "$env:NVIM_MACHINE_ROLE".Trim().ToLowerInvariant()
if ($role -eq 'workstation') { Fail 'this is the workstation: mobile builds never run here (MREL-06)' }
if ($role -notin @('', 'default')) { Fail "NVIM_MACHINE_ROLE is '$role', not a known machine: ask first (MASCHINE-01)" }
if ($role -eq '' -and ($env:USERDOMAIN -eq 'AzureAD' -or "$env:COMPUTERNAME".ToUpperInvariant().StartsWith('TRI-'))) {
  Fail 'NVIM_MACHINE_ROLE is empty but this looks like the workstation: set it first (MASCHINE-01)'
}
if (-not $Keystore) {
  Fail 'no keystore: set TESTING_ANDROID_DEV_KEYSTORE or pass -Keystore (the development key, docs/MOBILE.md "Signing")'
}
if (-not (Test-Path -LiteralPath $Keystore -PathType Leaf)) { Fail "the keystore '$Keystore' does not exist" }
$keystoreFull = (Resolve-Path -LiteralPath $Keystore).Path
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if ($keystoreFull.StartsWith($repoRoot, [StringComparison]::OrdinalIgnoreCase)) {
  Fail 'the keystore lies inside this repository: a key never lives in a repository (MREL-01)'
}

# The Android tools.
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$buildTools = Get-ChildItem -LiteralPath (Join-Path $sdk 'build-tools') -Directory -ErrorAction SilentlyContinue |
  Where-Object { Test-Path (Join-Path $_.FullName 'apksigner.bat') } |
  Sort-Object { [version]($_.Name -replace '[^0-9.].*$', '') } -Descending | Select-Object -First 1
if (-not $buildTools) { Fail "no Android build-tools with apksigner under '$sdk' (set ANDROID_HOME)" }
$apksigner = Join-Path $buildTools.FullName 'apksigner.bat'
$zipalign = Join-Path $buildTools.FullName 'zipalign.exe'
if (-not (Test-Path $zipalign)) { Fail "no zipalign in '$($buildTools.FullName)'" }

$null = New-Item -ItemType Directory -Force $OutDir
# The lowest Android version of the app (Tauri 2 default); given to apksigner so it signs and checks without reading the manifest.
$minSdk = '24'

# 2. The commit, and its CI.
if (-not $Apk) {
  Invoke-Native git -C $repoRoot fetch --quiet origin main
  if (-not $Sha) { $Sha = (git -C $repoRoot rev-parse origin/main).Trim() }
  else { $Sha = (git -C $repoRoot rev-parse --verify "$Sha^{commit}").Trim() }
} elseif (-not $Sha) {
  $Sha = '0000000000000000000000000000000000000000'
}
$short = $Sha.Substring(0, 7)
if ($Upload) {
  $runs = gh run list -R $Repo --commit $Sha --workflow ci --json status,conclusion | ConvertFrom-Json
  if (-not ($runs | Where-Object { $_.status -eq 'completed' -and $_.conclusion -eq 'success' })) {
    Fail "the CI of $short is not green: only a green commit of main is published (GIT-08)" 1
  }
}

# 3. Build, in a temporary worktree of the commit.
$unsigned = $Apk
if (-not $unsigned) {
  $src = Join-Path $OutDir "src-$short"
  if (Test-Path $src) { Invoke-Native git -C $repoRoot worktree remove --force $src }
  Say "building $short in $src"
  Invoke-Native git -C $repoRoot worktree add --quiet --detach $src $Sha
  try {
    $ndkRoot = Join-Path $sdk 'ndk'
    $ndk = Get-ChildItem -LiteralPath $ndkRoot -Directory | Sort-Object Name -Descending | Select-Object -First 1
    if (-not $ndk) { Fail "no NDK under '$ndkRoot'" }
    $env:ANDROID_HOME = $sdk
    $env:NDK_HOME = $ndk.FullName
    Push-Location (Join-Path $src 'ui')
    try { Invoke-Native pnpm install --frozen-lockfile; Invoke-Native pnpm mobile:build } finally { Pop-Location }
    Push-Location (Join-Path $src 'mobile')
    try { Invoke-Native cargo tauri android build --target aarch64 --apk } finally { Pop-Location }
    $built = Get-ChildItem -Recurse -Filter '*.apk' -LiteralPath (Join-Path $src 'mobile\gen\android\app\build\outputs\apk') |
      Where-Object { $_.Name -match 'release' } | Select-Object -First 1
    if (-not $built) { Fail 'the build made no release APK' 1 }
    $unsigned = Join-Path $OutDir "unsigned-$short.apk"
    Copy-Item -LiteralPath $built.FullName -Destination $unsigned -Force
  } finally {
    Invoke-Native git -C $repoRoot worktree remove --force $src
  }
}
if (-not (Test-Path -LiteralPath $unsigned -PathType Leaf)) { Fail "the APK '$unsigned' does not exist" }

# 4. Align, sign, verify.
$name = "testing-mobile-dev-$short.apk"
$aligned = Join-Path $OutDir "aligned-$short.apk"
$signed = Join-Path $OutDir $name
Remove-Item -LiteralPath $aligned, $signed -Force -ErrorAction SilentlyContinue
Invoke-Native $zipalign -f -p 4 $unsigned $aligned
if ($KeyEnvVar) {
  $plain = [Environment]::GetEnvironmentVariable($KeyEnvVar)
  if (-not $plain) { Fail "the environment variable '$KeyEnvVar' is empty" }
} else {
  $secret = Read-Host "Password of the keystore $(Split-Path -Leaf $keystoreFull)" -AsSecureString
  $plain = [Net.NetworkCredential]::new('', $secret).Password
}
# The password goes through stdin: not through the command line, an environment variable or a file.
$plain | & $apksigner sign --ks $keystoreFull --ks-key-alias $Alias --ks-pass stdin --min-sdk-version $minSdk --out $signed $aligned
if ($LASTEXITCODE -ne 0) { Fail 'apksigner could not sign (wrong password or alias?)' 1 }
$plain = $null
Remove-Item -LiteralPath $aligned -Force -ErrorAction SilentlyContinue
$certs = & $apksigner verify --min-sdk-version $minSdk --print-certs $signed
if ($LASTEXITCODE -ne 0) { Fail 'the signature does not verify' 1 }
$cert = ($certs | Where-Object { $_ -match 'certificate SHA-256 digest:\s*([0-9a-fA-F]+)' } | Select-Object -First 1) -replace '^.*digest:\s*', ''
if (-not $cert) { Fail 'apksigner printed no certificate digest' 1 }
if ($CertSha256 -and (($CertSha256 -replace '[:\s]', '') -ne $cert)) {
  Remove-Item -LiteralPath $signed -Force
  Fail "the APK is signed with the certificate $cert, not the expected one: wrong keystore, nothing kept" 1
}
Say "signed $name, certificate SHA-256 $cert"

# 5. The record beside it.
$apkSha = (Get-FileHash -Algorithm SHA256 -LiteralPath $signed).Hash.ToLowerInvariant()
$info = [ordered]@{
  schema = 'testing-android-dev/1'
  commit = $Sha
  apk = $name
  apk_sha256 = $apkSha
  apk_bytes = (Get-Item -LiteralPath $signed).Length
  certificate_sha256 = $cert
  built_at = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
}
$infoPath = Join-Path $OutDir 'android.json'
[IO.File]::WriteAllText($infoPath, ($info | ConvertTo-Json) + "`n", [Text.UTF8Encoding]::new($false))
$infoSha = (Get-FileHash -Algorithm SHA256 -LiteralPath $infoPath).Hash.ToLowerInvariant()
$sumsPath = Join-Path $OutDir 'SHA256SUMS-android'
[IO.File]::WriteAllText($sumsPath, "$apkSha  $name`n$infoSha  android.json`n", [Text.UTF8Encoding]::new($false))

# 6. Publish, only when asked.
$notes = "Development build of the Android app (testing mobil), signed with the development key of the maintainer. Not a release (MREL-02): it is built by hand on a private machine from a green commit of main. Install with `adb install -r $name`. The commit and the checksums are in android.json."
if (-not $Upload) {
  Say "not published (no -Upload). With -Upload it would run:"
  Say "  gh release create $Tag --repo $Repo --prerelease --latest=false   (only if it does not exist)"
  Say "  gh release upload $Tag $name android.json SHA256SUMS-android --repo $Repo --clobber"
  Say "  gh release delete-asset $Tag <every older testing-mobile-dev-*.apk> --repo $Repo --yes"
  Say "the files are in $OutDir"
  exit 0
}
gh release view $Tag --repo $Repo *> $null
if ($LASTEXITCODE -ne 0) {
  Invoke-Native gh release create $Tag --repo $Repo --prerelease --latest=false --title 'Android development build' --notes $notes
} else {
  Invoke-Native gh release edit $Tag --repo $Repo --prerelease --latest=false --notes $notes
}
Invoke-Native gh release upload $Tag $signed $infoPath $sumsPath --repo $Repo --clobber
$assets = gh release view $Tag --repo $Repo --json assets --jq '.assets[].name'
foreach ($old in ($assets | Where-Object { $_ -like 'testing-mobile-dev-*.apk' -and $_ -ne $name })) {
  Invoke-Native gh release delete-asset $Tag $old --repo $Repo --yes
}
Say "published $name on the pre-release $Tag of $Repo"

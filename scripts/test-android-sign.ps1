<#
.SYNOPSIS
Test of android-sign-and-upload.ps1 with a throwaway key: the refusals, the signing, the checks of the certificate.

.DESCRIPTION
Makes its own key (keytool) and its own stand-in for an unsigned APK in a temporary folder, and runs the script
against them. It never touches the development key of the maintainer and never publishes anything: the one test of
`-Upload` runs against a fake `gh` that answers "no green run". Needs pwsh 7, keytool and the Android build-tools
(apksigner, zipalign); without them it says `n/a` (CI-16).
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script = Join-Path $PSScriptRoot 'android-sign-and-upload.ps1'
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$tools = Get-ChildItem -LiteralPath (Join-Path $sdk 'build-tools') -Directory -ErrorAction SilentlyContinue |
  Where-Object { Test-Path (Join-Path $_.FullName 'apksigner.bat') }
$aapt2 = $tools | Sort-Object Name -Descending | ForEach-Object { Join-Path $_.FullName 'aapt2.exe' } | Where-Object { Test-Path $_ } | Select-Object -First 1
$androidJar = Get-ChildItem -Path (Join-Path $sdk 'platforms\android-*\android.jar') -ErrorAction SilentlyContinue |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not (Get-Command keytool -ErrorAction SilentlyContinue) -or -not $tools -or -not $aapt2 -or -not $androidJar) {
  Write-Host 'n/a  keytool, the Android build-tools (with aapt2) or an Android platform are missing on this machine'
  return
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("android-sign-test-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
$null = New-Item -ItemType Directory -Force $work
$failed = 0
function Check([string]$Name, [bool]$Ok, [string]$Detail = '') {
  if ($Ok) { Write-Host "ok   $Name" } else { Write-Host "FAIL $Name $Detail"; $script:failed++ }
}
function Run([string[]]$Arguments, [hashtable]$Env = @{}) {
  # The script in a child process: its exit code is the answer, and its environment is exactly what the test sets.
  $old = @{}
  foreach ($k in $Env.Keys) { $old[$k] = [Environment]::GetEnvironmentVariable($k); [Environment]::SetEnvironmentVariable($k, $Env[$k]) }
  try {
    $out = & pwsh -NoProfile -File $script @Arguments 2>&1 | Out-String
    return [pscustomobject]@{ Code = $LASTEXITCODE; Out = $out }
  } finally {
    foreach ($k in $old.Keys) { [Environment]::SetEnvironmentVariable($k, $old[$k]) }
  }
}

try {
  $pass = 'test-' + [guid]::NewGuid().ToString('N')
  $ks = Join-Path $work 'throwaway.jks'
  & keytool -genkeypair -keystore $ks -alias testing-dev -keyalg RSA -keysize 2048 -validity 30 `
    -storepass $pass -keypass $pass -dname 'CN=throwaway' 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0) { throw 'keytool could not make the throwaway key' }
  $other = Join-Path $work 'other.jks'
  & keytool -genkeypair -keystore $other -alias testing-dev -keyalg RSA -keysize 2048 -validity 30 `
    -storepass $pass -keypass $pass -dname 'CN=other' 2>&1 | Out-Null

  # A stand-in for an unsigned APK: aapt2 links an empty app (a manifest and nothing else), which apksigner accepts.
  $apk = Join-Path $work 'unsigned.apk'
  $manifest = Join-Path $work 'AndroidManifest.xml'
  Set-Content -LiteralPath $manifest -Value '<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="test.throwaway"><uses-sdk android:minSdkVersion="24"/><application/></manifest>'
  & $aapt2 link -o $apk --manifest $manifest -I $androidJar.FullName --min-sdk-version 24 --target-sdk-version 34 2>&1 | Out-Null
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path $apk)) { throw 'aapt2 could not make the stand-in APK' }

  $sha = '1234567890abcdef1234567890abcdef12345678'
  $passEnv = @{ TESTING_TEST_KEYPASS = $pass; TESTING_ANDROID_DEV_KEYSTORE = $null; NVIM_MACHINE_ROLE = 'default' }

  # The refusals come before anything slow.
  $r = Run @('-Apk', $apk, '-Sha', $sha, '-OutDir', "$work\o1") @{ TESTING_ANDROID_DEV_KEYSTORE = $null; NVIM_MACHINE_ROLE = 'default' }
  Check 'no keystore: refused with exit 2 and a message that names the variable' ($r.Code -eq 2 -and $r.Out -match 'TESTING_ANDROID_DEV_KEYSTORE') $r.Out

  $r = Run @('-Apk', $apk, '-Sha', $sha, '-Keystore', $ks, '-OutDir', "$work\o2") @{ NVIM_MACHINE_ROLE = 'workstation' }
  Check 'workstation: refused, nothing signed' ($r.Code -eq 2 -and $r.Out -match 'workstation' -and -not (Test-Path "$work\o2\testing-mobile-dev-1234567.apk")) $r.Out

  $r = Run @('-Apk', $apk, '-Sha', $sha, '-Keystore', $ks, '-OutDir', "$work\o2b") @{ NVIM_MACHINE_ROLE = 'something-else' }
  Check 'an unknown machine role: refused' ($r.Code -eq 2) $r.Out

  $inside = Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..')).Path 'throwaway-inside.jks'
  Copy-Item $ks $inside
  try {
    $r = Run @('-Apk', $apk, '-Sha', $sha, '-Keystore', $inside, '-OutDir', "$work\o3") @{ NVIM_MACHINE_ROLE = 'default' }
    Check 'a keystore inside the repository: refused' ($r.Code -eq 2 -and $r.Out -match 'inside this repository') $r.Out
  } finally { Remove-Item -LiteralPath $inside -Force -ErrorAction SilentlyContinue }

  # Signing with the throwaway key.
  $r = Run @('-Apk', $apk, '-Sha', $sha, '-Keystore', $ks, '-PasswordEnv', 'TESTING_TEST_KEYPASS', '-OutDir', "$work\o4") $passEnv
  $signed = "$work\o4\testing-mobile-dev-1234567.apk"
  Check 'signs the stand-in and names it after the commit' ($r.Code -eq 0 -and (Test-Path $signed)) $r.Out
  Check 'the password is not printed' (-not ($r.Out -like "*$pass*"))
  $info = if (Test-Path "$work\o4\android.json") { Get-Content -Raw "$work\o4\android.json" | ConvertFrom-Json } else { $null }
  Check 'android.json holds the commit, the file and a 64-digit certificate checksum' `
    ($null -ne $info -and $info.commit -eq $sha -and $info.apk -eq 'testing-mobile-dev-1234567.apk' -and $info.certificate_sha256 -match '^[0-9a-f]{64}$')
  if ($info) {
    $sums = Get-Content "$work\o4\SHA256SUMS-android"
    $real = (Get-FileHash -Algorithm SHA256 -LiteralPath $signed).Hash.ToLowerInvariant()
    Check 'SHA256SUMS-android lists the signed file with its real checksum' ($sums -contains "$real  testing-mobile-dev-1234567.apk")
    Check 'without -Upload nothing is published and the commands are only printed' ($r.Out -match 'not published')
  }

  # The certificate check: the right one passes, a wrong keystore fails and keeps nothing.
  $cert = $info.certificate_sha256
  $r = Run @('-Apk', $apk, '-Sha', $sha, '-Keystore', $ks, '-PasswordEnv', 'TESTING_TEST_KEYPASS', '-OutDir', "$work\o5", '-CertSha256', $cert.ToUpperInvariant()) $passEnv
  Check 'the expected certificate (any case) passes' ($r.Code -eq 0) $r.Out
  $r = Run @('-Apk', $apk, '-Sha', $sha, '-Keystore', $other, '-PasswordEnv', 'TESTING_TEST_KEYPASS', '-OutDir', "$work\o6", '-CertSha256', $cert) $passEnv
  Check 'a wrong keystore: exit 1, the APK is not kept' ($r.Code -eq 1 -and -not (Test-Path "$work\o6\testing-mobile-dev-1234567.apk")) $r.Out
  $r = Run @('-Apk', $apk, '-Sha', $sha, '-Keystore', $ks, '-PasswordEnv', 'TESTING_TEST_KEYPASS', '-Alias', 'nope', '-OutDir', "$work\o7") $passEnv
  Check 'a wrong alias: exit 1' ($r.Code -eq 1) $r.Out

  # -Upload with a fake gh that knows no green run: refused before anything is built or published.
  $fake = Join-Path $work 'fakebin'
  $null = New-Item -ItemType Directory -Force $fake
  Set-Content -LiteralPath (Join-Path $fake 'gh.cmd') -Value "@echo off`r`necho []`r`nexit /b 0"
  $withFake = $passEnv + @{ PATH = "$fake;$env:PATH" }
  $r = Run @('-Apk', $apk, '-Sha', $sha, '-Keystore', $ks, '-PasswordEnv', 'TESTING_TEST_KEYPASS', '-OutDir', "$work\o8", '-Upload') $withFake
  Check '-Upload of a commit without a green CI: refused, nothing signed' `
    ($r.Code -eq 1 -and $r.Out -match 'not green' -and -not (Test-Path "$work\o8\testing-mobile-dev-1234567.apk")) $r.Out
} finally {
  Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($failed -gt 0) { Write-Host "$failed check(s) failed"; exit 1 }
Write-Host 'all checks passed'

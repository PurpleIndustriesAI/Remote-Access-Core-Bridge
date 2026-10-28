# Builds the Remote Access Core (Bridge) Android app with the SDK's own tools only (no Gradle, no downloads).
# Needs JDK 17+, Android SDK platform 36 and build-tools 35+. Signing key and password file come
# from the environment and never live in the repository:
#   RAC_ANDROID_KEYSTORE   path to the .jks keystore (alias "rac")
#   RAC_ANDROID_KEYPASS    path to a file that holds the keystore password
param([string]$Out = (Join-Path $PSScriptRoot 'build\RemoteAccessCrew.apk'))
$ErrorActionPreference = 'Stop'
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$bt = Get-ChildItem (Join-Path $sdk 'build-tools') -Directory | Sort-Object { [version]$_.Name } | Select-Object -Last 1
$jar = Join-Path $sdk 'platforms\android-36\android.jar'
foreach ($p in @($bt.FullName, $jar)) { if (-not (Test-Path $p)) { throw "Missing Android SDK part: $p" } }
if (-not $env:RAC_ANDROID_KEYSTORE -or -not $env:RAC_ANDROID_KEYPASS) { throw 'Set RAC_ANDROID_KEYSTORE and RAC_ANDROID_KEYPASS to sign the app.' }

$work = Join-Path $PSScriptRoot 'build\tmp'
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Path "$work\gen", "$work\classes", "$work\dex" -Force | Out-Null
function Run([string]$Exe) {
    & $Exe @args
    if ($LASTEXITCODE) { throw "$(Split-Path -Leaf $Exe) failed ($LASTEXITCODE)" }
}
$tool = { param($n) Join-Path $bt.FullName $n }

Run (& $tool 'aapt2.exe') compile --dir (Join-Path $PSScriptRoot 'res') -o "$work\res.zip"
Run (& $tool 'aapt2.exe') link -I $jar --manifest (Join-Path $PSScriptRoot 'AndroidManifest.xml') `
    --min-sdk-version 34 --target-sdk-version 36 --version-code 3 --version-name 0.4.3 `
    --java "$work\gen" -o "$work\base.apk" "$work\res.zip"
$sources = @(Get-ChildItem (Join-Path $PSScriptRoot 'src'), "$work\gen" -Recurse -Filter *.java | ForEach-Object FullName)
Run javac -source 17 -target 17 -encoding UTF-8 '-Xlint:-options' -Werror -cp $jar -d "$work\classes" @sources
$classes = @(Get-ChildItem "$work\classes" -Recurse -Filter *.class | ForEach-Object FullName)
Run (& $tool 'd8.bat') --release --min-api 34 --lib $jar --output "$work\dex" @classes
Push-Location "$work\dex"
try { Run jar uf "$work\base.apk" classes.dex } finally { Pop-Location }
Run (& $tool 'zipalign.exe') -f -p 4 "$work\base.apk" "$work\aligned.apk"
New-Item -ItemType Directory -Path (Split-Path -Parent $Out) -Force | Out-Null
Run (& $tool 'apksigner.bat') sign --ks $env:RAC_ANDROID_KEYSTORE --ks-key-alias rac --ks-pass "file:$env:RAC_ANDROID_KEYPASS" --out $Out "$work\aligned.apk"
Run (& $tool 'apksigner.bat') verify $Out
"$((Get-FileHash $Out).Hash)  $(Split-Path -Leaf $Out)"

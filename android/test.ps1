# Runs android/test/RacTest.java on the desktop JVM against the Android SDK's stub jar.
$ErrorActionPreference = 'Stop'
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$jar = Join-Path $sdk 'platforms\android-36\android.jar'
$out = Join-Path $PSScriptRoot 'build\test'
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
$src = @(Join-Path $PSScriptRoot 'src\com\purpleindustries\rac\Rac.java'; Join-Path $PSScriptRoot 'test\RacTest.java')
javac -source 17 -target 17 '-Xlint:-options' -encoding UTF-8 -cp $jar -d $out @src
if ($LASTEXITCODE) { throw 'javac failed' }
java -cp "$out;$jar" com.purpleindustries.rac.RacTest
exit $LASTEXITCODE

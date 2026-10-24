# Checks the elevation guard and the suite guard. Touches no Sunshine or bridge state.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path 'install\host-moonlight-link.ps1'), [ref]$null, [ref]$null)
$fn = $ast.FindAll({ $args[0] -is [Management.Automation.Language.FunctionDefinitionAst] -and $args[0].Name -eq 'Assert-AdminOnly' }, $true)[0]
. ([scriptblock]::Create($fn.Extent.Text))
$tmp = Join-Path $env:TEMP "rac-adminonly-$PID.ps1"; Set-Content $tmp 'x'
try { Assert-AdminOnly $tmp; throw 'FAIL: user-writable file accepted' } catch { if ($_.Exception.Message -like 'FAIL*') { throw } } finally { Remove-Item $tmp }
'PASS: user-writable helper refused'
Assert-AdminOnly (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe')
'PASS: admin-only file accepted'
$old = $env:RAC_HOME
$env:RAC_HOME = Join-Path $env:TEMP "rac-suite-$PID"; New-Item -ItemType Directory $env:RAC_HOME | Out-Null
try { & .\install\install.ps1 -NoStart -NoCrewApp; throw 'FAIL: install ran under the suite' }
catch { if ($_.Exception.Message -notlike '*Remote Access Suite manages*') { throw } }
finally { Remove-Item $env:RAC_HOME; $env:RAC_HOME = $old }
'PASS: install.ps1 refuses a suite-managed bridge'

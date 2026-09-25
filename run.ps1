param([Parameter(ValueFromRemainingArguments=$true)][string[]]$LabArgs)
$ErrorActionPreference = 'Stop'
$env:PYTHONUTF8 = '1'
$env:PYTHONIOENCODING = 'utf-8'
$env:PYTHONHASHSEED = '0'
$python = Join-Path $PSScriptRoot '.venv-doubles\Scripts\python.exe'
if (!(Test-Path -LiteralPath $python)) { throw 'Run setup.ps1 first.' }
& $python -X utf8 (Join-Path $PSScriptRoot 'scripts\lab.py') @LabArgs
exit $LASTEXITCODE

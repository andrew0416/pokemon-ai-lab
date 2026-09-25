param([string]$Python = '3.12')
$ErrorActionPreference = 'Stop'
$env:PYTHONUTF8 = '1'
$env:PYTHONIOENCODING = 'utf-8'
$env:UV_CACHE_DIR = Join-Path $PSScriptRoot '.cache\uv'
$env:UV_PYTHON_INSTALL_DIR = Join-Path $PSScriptRoot '.runtime\python'
$env:npm_config_cache = Join-Path $PSScriptRoot '.cache\npm'
function CheckExit { if ($LASTEXITCODE -ne 0) { throw "Command failed: $LASTEXITCODE" } }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force vendor | Out-Null
    $sources = Get-Content source-lock.json -Raw | ConvertFrom-Json
    foreach ($entry in $sources.PSObject.Properties) {
        $repo = Join-Path $PSScriptRoot ('vendor\' + $entry.Name)
        if (-not (Test-Path -LiteralPath $repo)) {
            git init $repo
            CheckExit
            git -C $repo remote add origin $entry.Value.url
            CheckExit
            git -C $repo fetch --depth 1 origin $entry.Value.commit
            CheckExit
            git -C $repo checkout --detach FETCH_HEAD
            CheckExit
        } else {
            $actual = git -C $repo rev-parse HEAD
            CheckExit
            if ($actual -ne $entry.Value.commit) { throw "Version differs in $repo; preserving existing checkout." }
        }
    }
    npm.cmd ci --ignore-scripts --no-audit --no-fund
    CheckExit
    Push-Location vendor\pokemon-showdown
    try {
        npm.cmd ci --include=optional --ignore-scripts --no-audit --no-fund
        CheckExit
        node build
        CheckExit
        $config = Get-Content config\config.js -Raw
        $config = $config.Replace("exports.bindaddress = '0.0.0.0'", "exports.bindaddress = '127.0.0.1'")
        $config = $config.Replace('exports.lazysockets = true', 'exports.lazysockets = false')
        Set-Content config\config.js $config -Encoding utf8
    } finally { Pop-Location }
    if (-not (Test-Path .venv-doubles\Scripts\python.exe)) {
        uv venv --python $Python .venv-doubles
        CheckExit
    }
    uv pip sync --python .venv-doubles\Scripts\python.exe requirements-doubles.lock
    CheckExit
    uv pip install --python .venv-doubles\Scripts\python.exe --no-deps -e vendor\pokemon-vgc-ai
    CheckExit
    & .venv-doubles\Scripts\python.exe -X utf8 scripts\make_examples.py
    CheckExit
    node scripts\tooling.cjs import-ots teams\psyspam-popular.provenance.json
    CheckExit
    & .\run.ps1 doctor
    CheckExit
} finally { Pop-Location }


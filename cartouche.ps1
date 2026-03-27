$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Push-Location $ScriptDir

$VenvPath = Join-Path $ScriptDir ".venv"

if (-not (Test-Path $VenvPath)) {
    Write-Host "Creating virtual environment..." -ForegroundColor Cyan
    python -m venv .venv
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Error: Failed to create virtual environment." -ForegroundColor Red
        Read-Host "Press Enter to exit"
        Pop-Location
        exit 1
    }
    & .venv\Scripts\Activate.ps1
    if (Test-Path "requirements.txt") {
        Write-Host "Installing requirements..." -ForegroundColor Cyan
        pip install -q -r requirements.txt
    }
} else {
    & .venv\Scripts\Activate.ps1
}

Write-Host "Starting Cartouche..." -ForegroundColor Cyan
python cartouche.py $args

if ($LASTEXITCODE -ne 0) {
    Write-Host "`nScript execution failed. Check the output above for errors." -ForegroundColor Red
    Read-Host "Press Enter to exit"
}

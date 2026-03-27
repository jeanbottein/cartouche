@echo off
set SCRIPT_DIR=%~dp0
pushd "%SCRIPT_DIR%"

if not exist ".venv\" (
    echo Creating virtual environment...
    python -m venv .venv
    if errorlevel 1 (
        echo Error: Failed to create virtual environment.
        pause
        exit /b 1
    )
    call .venv\Scripts\activate
    if exist "requirements.txt" (
        echo Installing requirements...
        pip install -q -r requirements.txt
    )
) else (
    call .venv\Scripts\activate
)

python cartouche.py %*
if errorlevel 1 (
    echo.
    echo Script execution failed. Check the output above for errors.
    pause
)

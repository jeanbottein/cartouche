@echo off
REM Cartouche - Windows Batch Script
REM Runs the main Python script with proper error handling

python --version >nul 2>&1
if errorlevel 1 (
    echo Error: Python is not installed or not in PATH
    echo Please install Python 3.6+ from https://python.org
    pause
    exit /b 1
)

python cartouche.py %*
if errorlevel 1 (
    echo.
    echo Script execution failed. Check the output above for errors.
    pause
)

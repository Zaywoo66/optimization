@echo off
set "MODE=watch"
if /I "%~1"=="snapshot" set "MODE=snapshot"
if /I "%~1"=="compare" set "MODE=compare"
if exist "%~dp0bin\memory-pilot.exe" (
    "%~dp0bin\memory-pilot.exe" %MODE%
) else if exist "%~dp0target\release\memory-pilot.exe" (
    "%~dp0target\release\memory-pilot.exe" %MODE%
) else (
    if "%MODE%"=="compare" (
        echo Compare requires the Rust executable. 1>&2
        exit /b 1
    )
    if "%MODE%"=="snapshot" (
        powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0MemoryPilot.ps1" -SelfTest
    ) else (
        powershell.exe -NoProfile -ExecutionPolicy Bypass -STA -File "%~dp0MemoryPilot.ps1"
    )
)

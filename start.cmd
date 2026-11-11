@echo off
set "MODE=watch"
if /I "%~1"=="snapshot" set "MODE=snapshot"
if exist "%~dp0bin\memory-pilot.exe" (
    "%~dp0bin\memory-pilot.exe" %MODE%
) else if exist "%~dp0target\release\memory-pilot.exe" (
    "%~dp0target\release\memory-pilot.exe" %MODE%
) else (
    if "%MODE%"=="snapshot" (
        powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0MemoryPilot.ps1" -SelfTest
    ) else (
        powershell.exe -NoProfile -ExecutionPolicy Bypass -STA -File "%~dp0MemoryPilot.ps1"
    )
)

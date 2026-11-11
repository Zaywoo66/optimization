@echo off
if exist "%~dp0target\release\memory-pilot.exe" (
    "%~dp0target\release\memory-pilot.exe" watch
) else (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -STA -File "%~dp0MemoryPilot.ps1"
)

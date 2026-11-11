@echo off
set "MODE=watch"
if /I "%~1"=="snapshot" set "MODE=snapshot"
if /I "%~1"=="compare" set "MODE=compare"
if /I "%~1"=="report" set "MODE=report"
if exist "%~dp0bin\memory-pilot.exe" (
    "%~dp0bin\memory-pilot.exe" %MODE% %2
) else if exist "%~dp0target\release\memory-pilot.exe" (
    "%~dp0target\release\memory-pilot.exe" %MODE% %2
) else (
    echo Rust executable not found. Build with cargo build --release. 1>&2
    exit /b 1
)

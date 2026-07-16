@echo off
:: Install LuxePix to Start Menu
:: Creates a shortcut in the Start Menu Programs folder

set "EXE_PATH=%~dp0target\release\luxepix.exe"
set "ICON_PATH=%~dp0assets\icon.ico"
set "SHORTCUT_PATH=%APPDATA%\Microsoft\Windows\Start Menu\Programs\LuxePix.lnk"

if not exist "%EXE_PATH%" (
    echo ERROR: luxepix.exe not found at %EXE_PATH%
    echo Please run: cargo build --release
    pause
    exit /b 1
)

:: Create shortcut using PowerShell
powershell -NoProfile -Command ^
  "$ws = New-Object -COM WScript.Shell; ^
   $sc = $ws.CreateShortcut('%SHORTCUT_PATH%'); ^
   $sc.TargetPath = '%EXE_PATH%'; ^
   $sc.WorkingDirectory = '%~dp0'; ^
   $sc.IconLocation = '%ICON_PATH%'; ^
   $sc.Description = 'LuxePix - Image Browser'; ^
   $sc.Save()"

if exist "%SHORTCUT_PATH%" (
    echo ✓ LuxePix shortcut created in Start Menu!
    echo   Path: %SHORTCUT_PATH%
    echo.
    echo You can now find "LuxePix" in the Start Menu.
    echo To pin to taskbar: Right-click the shortcut ^> Pin to taskbar
) else (
    echo ERROR: Failed to create shortcut
)

pause

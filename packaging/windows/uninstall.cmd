@echo off
rem Removes what install.cmd installed. Settings, playlists and the sign-in
rem in %APPDATA%\spotless are kept.
setlocal
set "dir=%LOCALAPPDATA%\Programs\Spotless"
set "shortcut=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Spotless.lnk"
set "key=HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Spotless"

rem Carry on from a copy in %TEMP%, so this script's folder can go too.
if /i not "%~dp0"=="%TEMP%\" (
    copy /y "%~f0" "%TEMP%\spotless-uninstall.cmd" >nul
    "%TEMP%\spotless-uninstall.cmd"
)

if exist "%dir%\Spotless.exe" del "%dir%\Spotless.exe" 2>nul
if exist "%dir%\Spotless.exe" (
    echo Spotless is running. Quit it ^(its tray icon, then Quit; or Ctrl+Q^)
    echo and run this again.
    pause
    exit /b 1
)
if exist "%shortcut%" del "%shortcut%"
reg delete "%key%" /f >nul 2>&1

if exist "%dir%" rmdir /s /q "%dir%"
echo Removed Spotless (your settings and playlists are kept).
pause

@echo off
rem Installs Spotless for the current user, no administrator needed:
rem   %LOCALAPPDATA%\Programs\Spotless\Spotless.exe and uninstall.cmd
rem   a Start menu shortcut
rem   an entry in Settings, Apps, to uninstall it from there
rem Run it again to update. Settings, playlists and the sign-in live in
rem %APPDATA%\spotless and are kept.
setlocal
set "here=%~dp0"
set "dir=%LOCALAPPDATA%\Programs\Spotless"
set "shortcut=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Spotless.lnk"
set "key=HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Spotless"

rem Errorlevel checks rather than ||, which Wine's cmd gets wrong.
if not exist "%dir%" mkdir "%dir%"
if not exist "%dir%" goto failed
copy /y "%here%Spotless.exe" "%dir%\Spotless.exe" >nul
if errorlevel 1 goto running
copy /y "%here%uninstall.cmd" "%dir%\uninstall.cmd" >nul
if errorlevel 1 goto failed

rem The shortcut through PowerShell, which reads the paths from the
rem environment, so no quoting can go wrong.
powershell -NoProfile -ExecutionPolicy Bypass -Command "$link = (New-Object -ComObject WScript.Shell).CreateShortcut($env:shortcut); $link.TargetPath = Join-Path $env:dir 'Spotless.exe'; $link.WorkingDirectory = $env:dir; $link.Description = 'A minimal Spotify player'; $link.Save()" >nul 2>&1
if errorlevel 1 echo The Start menu shortcut couldn't be made.

reg add "%key%" /f /v DisplayName /d "Spotless" >nul
reg add "%key%" /f /v DisplayVersion /d "@VERSION@" >nul
reg add "%key%" /f /v DisplayIcon /d "%dir%\Spotless.exe" >nul
reg add "%key%" /f /v InstallLocation /d "%dir%" >nul
reg add "%key%" /f /v UninstallString /d "\"%dir%\uninstall.cmd\"" >nul
reg add "%key%" /f /v NoModify /t REG_DWORD /d 1 >nul
reg add "%key%" /f /v NoRepair /t REG_DWORD /d 1 >nul

echo Installed Spotless @VERSION@ to %dir%
echo Start it from the Start menu.
pause
exit /b 0

:running
echo Spotless.exe couldn't be replaced. If Spotless is running, quit it
echo (its tray icon, then Quit; or Ctrl+Q) and run this again.
:failed
echo Spotless was not installed.
pause
exit /b 1

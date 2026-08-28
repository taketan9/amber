@echo off
setlocal enabledelayedexpansion
chcp 65001 > nul
rem cian (Electron) を起動する。
rem
rem Electron の置き場所を毎回打つのをやめるためのもの。よくある場所を順に
rem 探し、見つからなければ「どこを探したか」を言って終わる ―― 黙って失敗する
rem のが一番たちが悪いので。

set "HERE=%~dp0"
set "HERE=%HERE:~0,-1%"
set "FOUND="

rem 1. 環境変数で決め打ちされていれば、それが最優先
if defined CIAN_ELECTRON (
    if exist "%CIAN_ELECTRON%" set "FOUND=%CIAN_ELECTRON%"
)

rem 2. リポジトリの隣に展開した配布版（社内はこの形）
if not defined FOUND (
    for /d %%D in ("%HERE%\..\..\electron-v*") do (
        if exist "%%D\electron.exe" set "FOUND=%%D\electron.exe"
    )
)

rem 3. npm で入れた場合
if not defined FOUND (
    if exist "%HERE%\node_modules\electron\dist\electron.exe" (
        set "FOUND=%HERE%\node_modules\electron\dist\electron.exe"
    )
)

if not defined FOUND (
    echo Electron が見つかりません。探した場所:
    echo   1. %%CIAN_ELECTRON%%  (いまの値: "%CIAN_ELECTRON%"^)
    echo   2. %HERE%\..\..\electron-v*\electron.exe
    echo   3. %HERE%\node_modules\electron\dist\electron.exe
    echo.
    echo 配布版を展開した場所を指定するなら:
    echo   set CIAN_ELECTRON=C:\path\to\electron-v33.4.11-win32-x64\electron.exe
    pause
    exit /b 1
)

rem エンジンが無ければ、起動してから空の窓を見せるより先に言う。
if not exist "%HERE%\cian-server.exe" (
    if not exist "%HERE%\..\target\release\cian-server.exe" (
        if not exist "%HERE%\..\target\debug\cian-server.exe" (
            echo cian-server.exe がありません。次のどちらかを:
            echo   - リリースの cian-server-win-x64.exe を "%HERE%\cian-server.exe" に置く
            echo   - cargo build --release -p cian-server
            pause
            exit /b 1
        )
    )
)

echo Electron: %FOUND%
"%FOUND%" "%HERE%" %*

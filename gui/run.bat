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

rem 2. 隣の electron.txt に書いてあれば、それ。
rem
rem このファイルは配布物に入れていない ―― 入れると、次の版を展開したときに
rem 上書きされてしまう。run.bat を直接書き換えるより、こちらのほうが更新に
rem 耐える。中身は Electron の置き場所を1行書くだけ。exe まででも、それが
rem 入っているフォルダまででも受ける。; で始まる行は覚え書きとして読み飛ばす。
if not defined FOUND (
    if exist "%HERE%\electron.txt" (
        for /f "usebackq delims=" %%L in ("%HERE%\electron.txt") do (
            if not defined FOUND (
                if exist "%%~L\electron.exe" (
                    set "FOUND=%%~L\electron.exe"
                ) else if exist "%%~L" (
                    set "FOUND=%%~L"
                )
            )
        )
    )
)

rem 3. すぐ隣、または1つ上に展開した配布版（zip を並べて置いた形）
if not defined FOUND (
    for /d %%D in ("%HERE%\electron-v*" "%HERE%\..\electron-v*") do (
        if exist "%%D\electron.exe" set "FOUND=%%D\electron.exe"
    )
)

rem 4. リポジトリの隣に展開した配布版（ソースから開発している形）
if not defined FOUND (
    for /d %%D in ("%HERE%\..\..\electron-v*") do (
        if exist "%%D\electron.exe" set "FOUND=%%D\electron.exe"
    )
)

rem 5. npm で入れた場合
if not defined FOUND (
    if exist "%HERE%\node_modules\electron\dist\electron.exe" (
        set "FOUND=%HERE%\node_modules\electron\dist\electron.exe"
    )
)

if not defined FOUND (
    echo Electron が見つかりません。探した場所:
    echo   1. %%CIAN_ELECTRON%%  (いまの値: "%CIAN_ELECTRON%"^)
    echo   2. %HERE%\electron.txt  に書かれた場所
    echo   3. %HERE%\electron-v*\electron.exe  と  %HERE%\..\electron-v*\electron.exe
    echo   4. %HERE%\..\..\electron-v*\electron.exe
    echo   5. %HERE%\node_modules\electron\dist\electron.exe
    echo.
    echo 一番手軽なのは、このフォルダに electron.txt を作って1行書くことです:
    echo   D:\apps\electron-v33.4.11-win32-x64
    echo.
    echo メモ帳で作れます。次の版を展開しても消えません
    echo ^(run.bat を直接書き換えると、そちらは上書きされます^)。
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

@echo off
rem ---------------------------------------------------------------------------
rem OneNote → ambər の写しを一回ぶん。1時間ごとに回すのは
rem タスク スケジューラの仕事（docs\onenote.ja.md）。
rem
rem 手で試すときはこれをダブルクリック。定時で回すときは、
rem この .cmd ではなく pythonw.exe を直に呼ぶ ── そうしないと
rem **一時間ごとに黒い窓が開く**。
rem ---------------------------------------------------------------------------
setlocal

rem ここだけ自分に合わせて書き換える。
set "OUT=%USERPROFILE%\Documents\amber\OneNote"
set "LOGFILE=%LOCALAPPDATA%\onenote2md.log"

rem 記録は保存ディレクトリの外へ。ambər のペインは .md 以外のファイルも
rem 並べるので、中に置くとノートに混ざる。
if not exist "%OUT%" mkdir "%OUT%"

py -3 "%~dp0onenote2md.py" --out "%OUT%" --sync --prune --log "%LOGFILE%"
set RC=%ERRORLEVEL%

if not "%RC%"=="0" (
    echo.
    echo 落ちました。わけは "%LOGFILE%" の末尾に残っています。
)
exit /b %RC%

@echo off
rem amber の窓を、ソースから走らせる（Windows）。
rem
rem     gui\run.bat
rem
rem `run.sh` と同じことをする。片方だけ直すと、片方の機械でだけ動かなく
rem なるので、**足すときは両方に足す**。
rem
rem エンジンを先に建てるのは、`cargo test` が bin を更新しないから ──
rem 「直したのに効かない」の半分はこれ。
rem
rem `vendor\` も見る。Monaco も vim も図もそこに置いてあり、git には
rem 入れていない。無いまま起動すると窓は開くが中身が真っ白で、原因が
rem 「落としていない」だと画面のどこにも書いていない。
setlocal
cd /d "%~dp0.."

cargo build -p amber-server
if errorlevel 1 goto :fail

cd gui
if not exist node_modules (
    call npm install
    if errorlevel 1 goto :fail
)
if not exist vendor\monaco (
    call node vendor.js
    if errorlevel 1 goto :fail
)

call npm start
if errorlevel 1 goto :fail
endlocal
exit /b 0

:fail
echo.
echo 立ち上げに失敗しました。要るもの: rustup / cargo と Node.js。
echo   cargo  : https://rustup.rs
echo   node   : https://nodejs.org
endlocal
exit /b 1

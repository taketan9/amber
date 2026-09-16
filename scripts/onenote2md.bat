@echo off
rem OneNote を ambər に取り込む。**押すのは一回**（依頼 610）。
rem
rem   ダブルクリック          小さい窓が出る。選んで「取り込む」を押すだけ
rem   .onepkg を放り込む      窓を出さずに、そのまま取り込む
rem
rem 網に出られない端末で動く ── 要るのは Python だけ（pywin32 も 32bit も
rem 要らない。あれは OneNote に直接繋いでいた頃の話で、その道は外した）。
rem
rem **`chcp 65001` を先に打つ。** 日本語 Windows の既定は cp932 で、
rem セクションの名前が化けたまま画面に出る（本人の端末で出た）。
setlocal
chcp 65001 >nul 2>&1
set "HERE=%~dp0"

rem **Python の在り処は、ある順に探す。** `py` が入っていない端末がある。
set "PY="
where py >nul 2>&1 && set "PY=py -3"
if not defined PY (where python >nul 2>&1 && set "PY=python")
if not defined PY (
    echo Python が見つかりません。
    echo python.org の Windows installer を入れてから、もう一度押してください。
    pause
    exit /b 1
)

if "%~1"=="" (
    rem 押しただけ ── 窓を出す。
    %PY% "%HERE%onenote2md.py"
    exit /b %errorlevel%
)

rem 放り込まれた ── 既定の出力先へ、そのまま取り込む。
rem **ambər の既定の保存ディレクトリの下には掘らない**（本人が決めた）。
rem 隣に OneNote を作り、ambər 側で保存ディレクトリを一つ足してもらう。
set "OUT=%USERPROFILE%\Documents\OneNote"
:loop
if "%~1"=="" goto done
echo === %~1
%PY% "%HERE%onenote2md.py" --out "%OUT%" "%~1"
if errorlevel 1 set "BAD=1"
shift
goto loop
:done
echo.
if defined BAD (
    echo うまくいかなかったものがあります。上の行に理由が出ています。
) else (
    echo 取り込みました: %OUT%
    echo ambər の 設定 →「保存ディレクトリの追加・変更・削除」で、ここを足すと読めます。
)
pause

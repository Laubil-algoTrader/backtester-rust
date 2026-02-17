@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
cd /d "C:\Users\Lautaro\Documents\backtester-rust\src-tauri"
C:\Users\Lautaro\.cargo\bin\cargo test 2>&1

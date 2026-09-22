@echo off
rem Запускает СВЕЖУЮ сборку игры, с сейвами там же, где они у вас сейчас
rem (в папке репозитория). target\release обновляется каждой сборкой,
rem так что этот ярлык не может протухнуть, в отличие от папок в dist.
cd /d "%~dp0"
if not exist "target\release\primitive_client.exe" (
    echo Сборки нет. Выполните: cargo build --release -p primitive_client
    pause
    exit /b 1
)
start "" "target\release\primitive_client.exe"

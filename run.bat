@echo off
chcp 65001 >nul
rem 日本語パスではビルドできないため、テスト用の D:\quickfeed でビルド・起動する
rem ソースは D:\学校\app\my-quick-feed が正規（git管理）
echo [INFO] D:\quickfeed の run.bat を実行します（日本語パスではビルド不可のため）
call "D:\quickfeed\run.bat"

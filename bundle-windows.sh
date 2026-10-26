#!/bin/sh
# Builds dist/Doodoro-windows.zip: the release binary and the fonts' licences (the fonts themselves
# are compiled into the binary). Run from Git Bash. Unsigned, so SmartScreen warns on first launch.
set -e
cd "$(dirname "$0")"

cargo build --release

DIR="dist/Doodoro"
rm -rf "$DIR" dist/Doodoro-windows.zip
mkdir -p "$DIR"
cp target/release/doodoro.exe "$DIR/Doodoro.exe"
cp LICENSE fonts/OFL-*.txt "$DIR/"

cat > "$DIR/使用說明.txt" <<'EOF'
Doodoro 番茄鐘

1. 把整個資料夾解壓縮到任何地方（例如桌面）。
2. 雙擊 Doodoro.exe 開啟。
3. 第一次開啟時，Windows 可能會顯示「Windows 已保護您的電腦」。
   這是因為程式沒有數位簽章。按「其他資訊」→「仍要執行」即可。

使用方式
- 點上方的緞帶選擇：專注、短休息、長休息；用 − 和 + 調整分鐘數。
- 空白鍵開始或暫停；進行中按 r 重設、按 s 跳過。
- 按「–」縮成小膠囊，點膠囊再展開。拖曳卡片或右上角的 ⠿ 可以移動。
- 按「×」關閉。

設定存在 %APPDATA%\Doodoro，刪掉這個資料夾就會回到預設值。
需要 Windows 10（1809 以後）或 Windows 11。
EOF

powershell -NoProfile -Command "Compress-Archive -Path '$DIR' -DestinationPath 'dist/Doodoro-windows.zip'"
echo "built dist/Doodoro-windows.zip"

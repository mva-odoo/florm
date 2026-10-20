#!/bin/sh
# Compile Florm pour Windows depuis WSL / Linux et copie florm.exe dans un dossier Windows.
# Usage : scripts/build-windows.sh [dossier de destination]
# (par défaut : C:\Users\<toi>\Florm sous WSL)
set -e
cd "$(dirname "$0")/.."

rustup target add x86_64-pc-windows-gnu >/dev/null
if command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    cargo build --release --target x86_64-pc-windows-gnu
elif command -v cargo-zigbuild >/dev/null 2>&1 && python3 -c "import ziglang" 2>/dev/null; then
    # sans sudo : zig sert d'éditeur de liens Windows
    cargo zigbuild --release --target x86_64-pc-windows-gnu
else
    echo "Il faut un éditeur de liens Windows, au choix :" >&2
    echo "  sudo apt install mingw-w64" >&2
    echo "  ou, sans sudo : pip install --user ziglang && cargo install cargo-zigbuild" >&2
    exit 1
fi
exe=target/x86_64-pc-windows-gnu/release/florm.exe

dest="$1"
if [ -z "$dest" ] && command -v cmd.exe >/dev/null 2>&1; then
    user=$(cmd.exe /c "echo %USERNAME%" 2>/dev/null | tr -d '\r')
    dest="/mnt/c/Users/$user/Florm"
fi
if [ -n "$dest" ]; then
    mkdir -p "$dest"
    cp "$exe" "$dest/florm.exe"
    echo "Copié dans $dest/florm.exe"
    command -v wslpath >/dev/null 2>&1 && echo "Sous Windows : $(wslpath -w "$dest/florm.exe")"
else
    echo "Binaire : $exe"
fi

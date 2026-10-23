#!/bin/sh
# Installe ou met à jour Florm sur macOS, sans « Florm est endommagé » ni xattr à lancer :
#   curl -fsSL https://raw.githubusercontent.com/mva-odoo/florm/main/packaging/install.sh | sh
# Un fichier téléchargé par le navigateur reçoit l'attribut de quarantaine, que Gatekeeper
# vérifie au lancement ; curl ne le met pas. Le .dmg est vérifié avec le SHA256SUMS de la release.
set -eu
repo=mva-odoo/florm

fail() {
    echo "florm : $*" >&2
    exit 1
}

[ "$(uname -s)" = Darwin ] || fail "ce script est pour macOS (Linux : voir les paquets de la release)"

# …/releases/latest redirige vers …/releases/tag/v<version> (pas d'API GitHub : pas de limite)
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")
version=${tag##*/v}
[ -n "$version" ] && [ "$version" != "$tag" ] || fail "dernière version introuvable"
name="Florm-$version-macos-universal.dmg"
base="https://github.com/$repo/releases/download/v$version"

tmp=$(mktemp -d)
trap 'hdiutil detach -quiet "$tmp/mnt" 2>/dev/null || true; rm -rf "$tmp"' EXIT

echo "Téléchargement de Florm $version…"
curl -fL --progress-bar -o "$tmp/$name" "$base/$name"
curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"
expected=$(awk -v n="$name" '$2 == n || $2 == "*" n { print $1 }' "$tmp/SHA256SUMS")
actual=$(shasum -a 256 "$tmp/$name" | awk '{ print $1 }')
[ -n "$expected" ] && [ "$expected" = "$actual" ] || fail "$name est corrompu (somme SHA-256 différente de SHA256SUMS)"

dest=/Applications
[ -w "$dest" ] || dest="$HOME/Applications"
mkdir -p "$dest"
hdiutil attach -nobrowse -readonly -mountpoint "$tmp/mnt" "$tmp/$name" >/dev/null
rm -rf "$dest/Florm.app"
ditto "$tmp/mnt/Florm.app" "$dest/Florm.app"
echo "Florm $version est installé dans $dest/Florm.app"

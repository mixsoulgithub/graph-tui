#!/bin/sh
# graph-tui 安装脚本:下载 GitHub Release 的预编译包, 校验 sha256, 装到用户目录.
#
#   curl -LsSf https://raw.githubusercontent.com/mixsoulgithub/graph-tui/main/install.sh | sh
#   ... | sh -s -- --version 0.4.0
#   ... | sh -s -- --dir "$HOME/bin"
#
# 需要 curl, tar, 以及 sha256sum 或 shasum. 不需要 sudo.
set -eu

REPO=mixsoulgithub/graph-tui
VERSION=
DIR=${GRAPH_TUI_INSTALL_DIR:-$HOME/.local/bin}

usage() {
  cat <<'EOF'
usage: install.sh [--version <v>] [--dir <path>]

  --version <v>   install a specific release (for example 0.4.0); default: latest
  --dir <path>    install directory (default: $GRAPH_TUI_INSTALL_DIR or ~/.local/bin)
  -h, --help      show this help
EOF
}

die() {
  printf 'graph-tui: %s\n' "$1" >&2
  exit 1
}

while [ $# -gt 0 ]; do
  case $1 in
    --version)
      [ $# -ge 2 ] || die "--version needs a value"
      VERSION=$2
      shift 2
      ;;
    --dir)
      [ $# -ge 2 ] || die "--dir needs a value"
      DIR=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1 (try --help)"
      ;;
  esac
done

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v tar >/dev/null 2>&1 || die "tar is required"

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux)
    case "$arch" in
      x86_64|amd64) target=x86_64-unknown-linux-musl ;;
      *) die "unsupported Linux architecture: $arch (x86_64 only)" ;;
    esac
    ;;
  Darwin)
    case "$arch" in
      arm64|aarch64) target=aarch64-apple-darwin ;;
      x86_64) target=x86_64-apple-darwin ;;
      *) die "unsupported macOS architecture: $arch" ;;
    esac
    ;;
  *)
    die "unsupported OS: $os (Linux x86_64 or macOS)"
    ;;
esac

if [ -n "$VERSION" ]; then
  case "$VERSION" in
    v*) VERSION=${VERSION#v} ;;
  esac
  base="https://github.com/$REPO/releases/download/v$VERSION"
  label="v$VERSION"
else
  base="https://github.com/$REPO/releases/latest/download"
  label=latest
fi

archive="graph-tui-$target.tar.gz"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/graph-tui.XXXXXX") || die "cannot create a temporary directory"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

fetch() {
  curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 15 -o "$2" "$1"
}

printf 'graph-tui: downloading %s (%s)\n' "$label" "$target" >&2
fetch "$base/$archive" "$tmp/$archive" || die "cannot download $base/$archive"
fetch "$base/graph-tui-$target.sha256" "$tmp/graph-tui-$target.sha256" || die "cannot download the checksum"

want=$(awk 'NR == 1 { print $1 }' "$tmp/graph-tui-$target.sha256")
case "$want" in
  *[!0-9a-fA-F]*|'') die "invalid checksum file" ;;
esac
[ ${#want} -eq 64 ] || die "invalid checksum length"

if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum "$tmp/$archive")
else
  command -v shasum >/dev/null 2>&1 || die "sha256sum or shasum is required"
  got=$(shasum -a 256 "$tmp/$archive")
fi
got=${got%% *}
[ "$got" = "$want" ] || die "sha256 mismatch (expected $want, got $got)"

tar -xzf "$tmp/$archive" -C "$tmp" || die "cannot unpack $archive"
[ -f "$tmp/graph-tui" ] || die "archive does not contain graph-tui"

mkdir -p "$DIR" || die "cannot create $DIR"
install -m 755 "$tmp/graph-tui" "$DIR/graph-tui" || die "cannot install to $DIR"

printf 'installed %s\n' "$DIR/graph-tui" >&2
"$DIR/graph-tui" --version || true
case ":$PATH:" in
  *":$DIR:"*) ;;
  *) printf 'note: %s is not on PATH\n' "$DIR" >&2 ;;
esac

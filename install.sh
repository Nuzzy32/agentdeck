#!/bin/sh
# agentdeck installer: builds agentdeck from source and installs agentdeck.app (macOS).
#
#   curl -fsSL https://raw.githubusercontent.com/Nuzzy32/agentdeck/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/Nuzzy32/agentdeck/main/install.sh | sh -s -- --dry-run
#
# Options (or the matching environment variables):
#   --ref REF       branch or tag to build (AGENTDECK_REF, default: main)
#   --src DIR       where the source is kept (AGENTDECK_SRC, default: ~/.agentdeck/src)
#   --dest DIR      where agentdeck.app goes (AGENTDECK_DEST, default: /Applications if writable, else ~/Applications)
#   --dry-run       print what would run, change nothing
#
# Missing prerequisites are reported with the command to install them; this script never installs toolchains,
# never uses sudo, and never touches agent CLIs or their logins. There are no prebuilt releases yet (DECISIONS D-026).
set -eu

REPO="https://github.com/Nuzzy32/agentdeck.git"
NODE_MIN=22

say() { printf '%s\n' "agentdeck: $*"; }
die() {
  printf '%s\n' "agentdeck: $*" >&2
  exit 1
}

# Everything runs from main(), so a download cut short never executes half a script.
main() {
  ref="${AGENTDECK_REF:-main}"
  src="${AGENTDECK_SRC:-$HOME/.agentdeck/src}"
  dest="${AGENTDECK_DEST:-}"
  dry=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --ref) [ $# -ge 2 ] || die "--ref needs a value"; ref="$2"; shift 2 ;;
      --src) [ $# -ge 2 ] || die "--src needs a value"; src="$2"; shift 2 ;;
      --dest) [ $# -ge 2 ] || die "--dest needs a value"; dest="$2"; shift 2 ;;
      --dry-run) dry=1; shift ;;
      -h | --help) sed -n '2,15p' "$0" 2>/dev/null || say "see the header of install.sh"; exit 0 ;;
      *) die "unknown option: $1 (try --help)" ;;
    esac
  done
  case "$ref" in
    -* | *[!A-Za-z0-9._/-]*) die "--ref may only contain letters, digits, . _ / -" ;;
  esac

  [ "$(uname -s)" = "Darwin" ] || die "only macOS is supported for now; see docs/PLATFORMS.md for Windows and Linux."

  check_prereqs

  if [ -z "$dest" ]; then
    if [ -w /Applications ]; then dest=/Applications; else dest="$HOME/Applications"; fi
  fi
  app="$dest/agentdeck.app"

  if pgrep -f "agentdeck.app/Contents/MacOS/agentdeck" >/dev/null 2>&1; then
    msg="agentdeck is running. Quit it first (its panes and agents would stop mid-install), then run this again."
    if [ "$dry" -eq 1 ]; then say "warning: $msg"; else die "$msg"; fi
  fi

  say "source: $src (ref $ref)"
  say "install to: $app"
  fetch_source
  build
  install_app
  say "done. Open it with: open \"$app\""
}

run() {
  if [ "$dry" -eq 1 ]; then
    printf '  [dry-run] %s\n' "$*"
  else
    "$@"
  fi
}

check_prereqs() {
  missing=""
  xcode-select -p >/dev/null 2>&1 || missing="$missing\n  Xcode Command Line Tools: xcode-select --install"
  command -v git >/dev/null 2>&1 || missing="$missing\n  git: xcode-select --install"
  if command -v node >/dev/null 2>&1; then
    major="$(node -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0)"
    [ "$major" -ge "$NODE_MIN" ] 2>/dev/null || missing="$missing\n  Node.js $NODE_MIN+ (found $major): https://nodejs.org or brew install node"
  else
    missing="$missing\n  Node.js $NODE_MIN+: https://nodejs.org or brew install node"
  fi
  command -v pnpm >/dev/null 2>&1 || missing="$missing\n  pnpm: corepack enable pnpm   (or: npm install -g pnpm)"
  command -v cargo >/dev/null 2>&1 || missing="$missing\n  Rust (cargo): curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  if [ -n "$missing" ]; then
    printf 'agentdeck: missing prerequisites. Install these, open a new terminal, and run the installer again:%b\n' "$missing" >&2
    exit 1
  fi
}

fetch_source() {
  if [ -d "$src/.git" ]; then
    origin="$(git -C "$src" remote get-url origin 2>/dev/null || true)"
    [ "$origin" = "$REPO" ] || die "$src is a different repository ($origin). Pass --src to use another folder."
    if [ -n "$(git -C "$src" status --porcelain)" ]; then
      die "$src has local changes. Commit or remove them, or pass --src to use another folder."
    fi
    say "updating source"
    run git -C "$src" fetch --depth 1 origin "$ref"
    run git -C "$src" checkout --quiet --detach FETCH_HEAD
  elif [ -e "$src" ] && [ -n "$(ls -A "$src" 2>/dev/null)" ]; then
    die "$src exists and is not an agentdeck checkout. Pass --src to use another folder."
  else
    say "downloading source"
    run mkdir -p "$(dirname "$src")"
    run git clone --quiet --depth 1 --branch "$ref" "$REPO" "$src"
  fi
}

build() {
  say "building (first build takes a few minutes)"
  if [ "$dry" -eq 1 ]; then
    printf '  [dry-run] cd %s && pnpm install --frozen-lockfile && pnpm tauri build --bundles app\n' "$src"
    return
  fi
  (cd "$src" && pnpm install --frozen-lockfile && pnpm tauri build --bundles app)
}

install_app() {
  target_dir="${CARGO_TARGET_DIR:-$src/src-tauri/target}"
  built="$target_dir/release/bundle/macos/agentdeck.app"
  if [ "$dry" -eq 0 ] && [ ! -d "$built" ]; then
    die "build finished but $built is missing."
  fi
  say "installing agentdeck.app"
  run mkdir -p "$dest"
  # Copy next to the old app first, then swap, so a failed copy never leaves no app at all.
  run rm -rf "$dest/agentdeck.app.new"
  run ditto "$built" "$dest/agentdeck.app.new"
  run rm -rf "$dest/agentdeck.app"
  run mv "$dest/agentdeck.app.new" "$dest/agentdeck.app"
}

main "$@"

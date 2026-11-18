# Shared helpers for the stow drivers (stow-home.sh, stow-root.sh). POSIX sh,
# sourced, never executed. Lives at the stow/ module root, beside the
# drivers and outside every group, so it is structurally un-stowable.
# shellcheck shell=sh

# The running OS as a group suffix: linux, darwin.
stow_os() {
  uname -s | tr '[:upper:]' '[:lower:]'
}

# The groups a driver stows for target <base> (home|root), one per line:
# <base> itself (every OS) and <base>-<os> for the running OS, each only if
# it exists. A group for another OS is skipped by construction — there is
# no list of what belongs where; the directory a package sits in decides.
stow_groups_for() {
  for g in "$1" "$1-$(stow_os)"; do
    [ -d "$STOW_DIR/$g" ] && printf '%s\n' "$g"
  done
  return 0
}

# Remove DANGLING links a package's own files would occupy, when the link
# was made by stow for a package of the same name in any group — i.e. the
# link left behind when a package moves between groups (home/x ->
# home-linux/x). stow refuses to replace such a link ("existing target is
# not owned by stow") and aborts the whole package, so without this a
# package move strands every machine that stowed the old layout. Nothing
# else is touched: a live link, a real file, or a link that names any
# other path stays put and surfaces as a stow conflict, as it should.
prune_moved_links() { # <group-dir> <package> <target-root>
  ( cd "$1/$2" && find . \( -type f -o -type l \) -print ) | while IFS= read -r rel; do
    rel=${rel#./}
    t="${3%/}/$rel"
    [ -L "$t" ] && [ ! -e "$t" ] || continue
    case "$(readlink "$t")" in
      */stow/*/"$2/$rel")
        rm -f "$t" && echo "pruned stale link from a moved package: $t"
        ;;
    esac
  done
}

# Stow every package of one group into <target-root>.
stow_group() { # <group> <target-root>
  for pkg in "$STOW_DIR/$1"/*/; do
    [ -d "$pkg" ] || continue
    name=$(basename "$pkg")
    prune_moved_links "$STOW_DIR/$1" "$name" "$2"
    ( cd "$STOW_DIR/$1" && stow --no-folding --restow --target="$2" "$name" )
    echo "stowed: $1/$name -> $2"
  done
}

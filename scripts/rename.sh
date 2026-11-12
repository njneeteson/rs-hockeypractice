#!/usr/bin/env sh
# Turn this template into a named project.
#
#   ./scripts/rename.sh recipe-box "Recipe Box"
#
# Replaces the template's placeholder names in every tracked text file:
#
#   LocalApp   -> the display name           (e.g. "Recipe Box")
#   LOCALAPP   -> the environment prefix     (e.g. RECIPE_BOX)
#   localapp_  -> the Rust identifier prefix (e.g. recipe_box_)
#   localapp   -> the crate/image/URL name   (e.g. recipe-box)
#
# Run it once, from the repository root, right after creating the project.
set -eu

if [ $# -ne 2 ]; then
  echo "usage: $0 <kebab-name> <Display Name>" >&2
  exit 2
fi
name=$1
display=$2
if ! printf '%s' "$name" | grep -Eq '^[a-z][a-z0-9]*(-[a-z0-9]+)*$'; then
  echo "Name must be lowercase kebab-case, e.g. recipe-box (got '$name')." >&2
  exit 2
fi
snake=$(printf '%s' "$name" | tr '-' '_')
upper=$(printf '%s' "$snake" | tr '[:lower:]' '[:upper:]')

cd "$(dirname "$0")/.."
export NAME="$name" DISPLAY_NAME="$display" SNAKE="$snake" UPPER="$upper"
git ls-files --cached --others --exclude-standard \
  | grep -Ev '^scripts/rename\.(ps1|sh)$' \
  | grep -Ev '\.(png|jpg|jpeg|gif|ico|pdf|woff2?|sqlite)$' \
  | while IFS= read -r file; do
      [ -f "$file" ] || continue
      if grep -q -e LocalApp -e LOCALAPP -e localapp "$file"; then
        perl -pi -e 's/LocalApp/$ENV{DISPLAY_NAME}/g; s/LOCALAPP/$ENV{UPPER}/g; s/localapp_/$ENV{SNAKE}_/g; s/localapp/$ENV{NAME}/g' "$file"
        echo "updated $file"
      fi
    done

echo
echo "Renamed to $display ($name)."
echo "Next: cargo test --workspace, then edit README.md, the authors in Cargo.toml and LICENSE."

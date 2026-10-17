#!/bin/sh
# Trunk post_build hook: puts FIGCRAFT_BRAND_NAME in the static page's title and loading text.
# With the variable unset or empty the page is left as upstream's.
set -eu
[ -n "${FIGCRAFT_BRAND_NAME-}" ] || exit 0
page="${TRUNK_STAGING_DIR:?run by trunk}/index.html"

awk '
function literal(s, from, to,   out, at) {
  while ((at = index(s, from)) > 0) {
    out = out substr(s, 1, at - 1) to
    s = substr(s, at + length(from))
  }
  return out s
}
BEGIN {
  name = ENVIRON["FIGCRAFT_BRAND_NAME"]
  name = literal(name, "&", "&amp;")
  name = literal(name, "<", "&lt;")
  name = literal(name, ">", "&gt;")
  name = literal(name, "\"", "&quot;")
}
{
  line = literal($0, "<title>FigCraft</title>", "<title>" name "</title>")
  print literal(line, "Loading FigCraft&hellip;", "Loading " name "&hellip;")
}
' "$page" >"$page.branded"
mv "$page.branded" "$page"

if [ "$FIGCRAFT_BRAND_NAME" != FigCraft ] && grep -qF -e '<title>FigCraft</title>' -e 'Loading FigCraft&hellip;' "$page"; then
  echo "error: $page still says FigCraft" >&2
  exit 1
fi

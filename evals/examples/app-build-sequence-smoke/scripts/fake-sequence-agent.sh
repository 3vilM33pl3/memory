#!/usr/bin/env sh
set -eu

workspace="$1"
prompt_file="$2"
condition="$3"
marker=$(sed -n 's/.*STEP_MARKER: //p' "$prompt_file" | head -1)

if [ -z "$marker" ]; then
  marker="sequence-unknown"
fi

tmp="$workspace/index.html.tmp"
sed "s#</main>#  <section data-condition=\"$condition\">$marker</section>\\n    </main>#" \
  "$workspace/index.html" > "$tmp"
mv "$tmp" "$workspace/index.html"

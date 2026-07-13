#!/usr/bin/env bash
# Lint FontSpace markdown for hard-wrapped paragraphs.
#
# Repo rule (CLAUDE.md): markdown is never hard-wrapped — one logical line per
# paragraph, list item, and block-quote. A paragraph split across several lines
# carries no meaning and churns diffs.
#
# Heuristic: outside fenced code blocks, a non-blank line that is NOT a block
# starter (heading, list item, block-quote, table row, thematic break) but whose
# previous line was also non-blank is a continuation — i.e. a hard wrap. Separate
# blocks are always divided by a blank line in this style, so this does not fire
# on adjacent paragraphs or list items. Trailing "two-space" hard breaks are
# honoured (the following line is not flagged).
#
# Usage:
#   markdown-no-hardwrap.sh [FILE...]   # lint the given files
#   markdown-no-hardwrap.sh             # lint every *.md under FontSpace/
set -euo pipefail

cd "$(dirname "$0")/.."

if [ "$#" -gt 0 ]; then
  files=("$@")
else
  # shellcheck disable=SC2207
  files=($(find . -type d -name target -prune -o -type f -name '*.md' -print | sort))
fi

if [ "${#files[@]}" -eq 0 ]; then
  exit 0
fi

awk '
  FNR == 1 { in_code = 0; prev_nonblank = 0; prev_hardbreak = 0 }

  # Fenced code-block fences toggle code mode; the fence line itself is neutral.
  /^[ \t]*(```|~~~)/ { in_code = !in_code; prev_nonblank = 0; prev_hardbreak = 0; next }
  in_code           { prev_nonblank = 0; prev_hardbreak = 0; next }

  # Blank line ends the current block.
  /^[ \t]*$/ { prev_nonblank = 0; prev_hardbreak = 0; next }

  {
    starter = ($0 ~ /^[ \t]*#/)                 ||  # ATX heading
              ($0 ~ /^[ \t]*[-*+][ \t]/)        ||  # unordered list item
              ($0 ~ /^[ \t]*[0-9]+[.)][ \t]/)   ||  # ordered list item
              ($0 ~ /^[ \t]*>/)                 ||  # block-quote
              ($0 ~ /^[ \t]*\|/)                ||  # table row
              ($0 ~ /^[ \t]*\*\*/)              ||  # bold-label line (deliberate metadata/field line)
              ($0 ~ /^[ \t]*(-{3,}|={3,}|_{3,}|\*{3,})[ \t]*$/) # thematic break / setext / frontmatter

    if (prev_nonblank && !prev_hardbreak && !starter) {
      printf "%s:%d: hard-wrapped line (prose continues a non-blank line; join into one logical line)\n", FILENAME, FNR
      failed = 1
    }

    prev_nonblank = 1
    prev_hardbreak = ($0 ~ /  $/)   # trailing two spaces = intentional hard break
  }

  END { if (failed) exit 1 }
' "${files[@]}"

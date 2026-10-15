#!/usr/bin/env bash
# Builds the brand check page: every mark at 120 pixels, and the icon at the sizes a
# taskbar actually uses. Written by hand rather than by a tool so the page is a diff.
set -euo pipefail
out="target/preview/brand.html"
mkdir -p target/preview
{
  printf '%s\n' \
    '<!doctype html><meta charset=utf-8><title>Majestat brand</title>' \
    '<style>' \
    'body{margin:0;background:#f5f1e8;color:#0b0e14;font:12px ui-monospace,monospace}' \
    '.row{display:flex;flex-wrap:wrap;gap:14px;padding:14px;align-items:flex-end}' \
    'figure{margin:0;text-align:center}' \
    'figcaption{padding-top:4px}' \
    'svg{background:#0b0e14;border-radius:8px}' \
    '.large svg{width:120px;height:120px}' \
    'h2{margin:0;padding:16px 14px 0;font-size:13px;font-weight:600}' \
    '.note{margin:0;padding:6px 14px 0;max-width:56em;line-height:1.5}' \
    '.brand{width:300px;aspect-ratio:1024/559;background-repeat:no-repeat;background-position:center;background-size:contain}' \
    '.plate{background-color:#f5f1e8;border:1px solid rgba(11,14,20,.14)}' \
    '.ink{background-color:#0b0e14;border-radius:8px}' \
    '.inverted{filter:invert(1)}' \
    '</style><div class="row large">'
  for mark in crown sword mark icon; do
    printf '<figure>'
    tr -d '\n' < "assets/brand/${mark}.svg"
    printf '<figcaption>%s</figcaption></figure>' "$mark"
  done
  printf '%s\n' '</div><div class="row">'
  for size in 64 32 16; do
    printf '<figure><div style="width:%spx;height:%spx">' "$size" "$size"
    tr -d '\n' < assets/brand/icon.svg
    printf '<style>figure svg{width:100%%;height:100%%}</style></div><figcaption>icon %s</figcaption></figure>' "$size"
  done
  printf '%s\n' '</div>' \
    '<h2>logo</h2>' \
    '<p class="note">assets/brand/logo.png, supplied as a file rather than drawn here. One image, inlined once, shown on three grounds. It is 1024 by 559, eight bits per channel, RGB with no alpha, on a white field, so on the ink ground the field is a plate unless it is taken out.</p>' \
    '<div class="row">'
  logo=$(base64 -w0 assets/brand/logo.png)
  printf '%s\n' "<style>.logo{background-image:url(data:image/png;base64,${logo})}</style>"
  printf '%s\n' \
    '<figure><div class="brand plate logo"></div><figcaption>as supplied, on paper</figcaption></figure>' \
    '<figure><div class="brand ink logo"></div><figcaption>as supplied, on ink: the app ground</figcaption></figure>' \
    '<figure><div class="brand ink logo inverted"></div><figcaption>on ink, inverted</figcaption></figure>' \
    '</div>'
} > "$out"
echo "wrote $out"

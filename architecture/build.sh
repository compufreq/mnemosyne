#!/usr/bin/env sh
# Rebuild architecture/pdf/*.pdf AND the inlined copies in index.html from
# architecture/diagrams/*.svg. `diagrams/` is the single source; everything
# else here is derived, so edit an SVG and re-run this.
#
# The SVG sources are theme-aware: every colour is `var(--d-x, #light)` so the
# inlined copies in index.html follow the page theme. librsvg does NOT
# implement CSS custom properties — it renders unresolved var() as black — so
# the PDFs are produced from a flattened copy where each var() collapses to its
# light fallback. Print output is deliberately always light.
#
# **The inline step strips each diagram's own dark media query**, and that is
# not cosmetic. A standalone SVG needs it to be readable when opened directly,
# but inlined it sets `--d-*` on the `svg` element, which beats the `:root`
# values the page sets — so the diagram would follow the SYSTEM theme while the
# rest of the page follows the manual toggle, and the two would disagree.
# Inlining by hand reintroduces it every time; that is why this is a script.
#
# Run from the repo root:
#   docker run --rm -v "$PWD/architecture:/a" -w /a debian:bookworm-slim sh build.sh
set -eu

# Noto core + CJK are not decoration: the language diagrams carry Thai, Han,
# Kana and Devanagari, and without those families librsvg renders every one of
# them as a tofu box. The browser has the fonts and the PDF does not, so this
# is a defect only the PDF shows — check a rendered page, never just the SVG.
command -v rsvg-convert >/dev/null 2>&1 || {
  apt-get -qq update >/dev/null 2>&1
  apt-get -qq install -y librsvg2-bin python3 fonts-noto-core fonts-noto-cjk >/dev/null 2>&1
}

mkdir -p pdf .flat

python3 - <<'PY'
import glob, io, os, re

os.makedirs('.flat', exist_ok=True)
for p in sorted(glob.glob('diagrams/*.svg')):
    s = io.open(p, encoding='utf-8').read()
    # Innermost-first, repeatedly: var(--name, #rrggbb) -> #rrggbb.
    # Handles the nested var(--d-on, var(--d-card, #ffffff)) case.
    while 'var(' in s:
        new = re.sub(r'var\(\s*--[A-Za-z0-9-]+\s*,\s*(#[0-9a-fA-F]{6})\s*\)', r'\1', s)
        if new == s:
            break
        s = new
    # The dark media query is meaningless for print and only risks confusing
    # a renderer with partial CSS support.
    s = re.sub(r'\s*@media \(prefers-color-scheme: dark\) \{.*?\n  \}\n', '\n', s, flags=re.S)
    leftover = re.findall(r'var\(', s)
    if leftover:
        raise SystemExit('unflattened var() remains in %s' % p)
    io.open('.flat/' + os.path.basename(p), 'w', encoding='utf-8', newline='\n').write(s)
    print('flattened', p)
PY

for f in .flat/*.svg; do
  n=$(basename "$f" .svg)
  rsvg-convert -f pdf -o "pdf/$n.pdf" "$f"
  echo "pdf/$n.pdf"
done

rm -rf .flat

# Refresh every inlined copy in index.html from its source.
python3 - <<'PY'
import glob, io, os, re

idx_path = 'index.html'
idx = io.open(idx_path, encoding='utf-8').read()
for p in sorted(glob.glob('diagrams/*.svg')):
    name = os.path.basename(p)[:-4]
    slug = name.replace('-', '')
    svg = io.open(p, encoding='utf-8').read().strip()
    # Unique ids: several diagrams share `t`/`d` and duplicate ids in one
    # document make aria-labelledby resolve to whichever came first.
    svg = svg.replace('aria-labelledby="t d"', 'aria-labelledby="t_%s d_%s"' % (slug, slug))
    svg = svg.replace('<title id="t">', '<title id="t_%s">' % slug)
    svg = svg.replace('<desc id="d">', '<desc id="d_%s">' % slug)
    # See the header: inlined, this block would outrank the page's own theme.
    svg = re.sub(r'\s*@media \(prefers-color-scheme: dark\) \{.*?\n  \}\n', '\n', svg, flags=re.S)
    pat = re.compile(r'<svg [^>]*aria-labelledby="t_%s d_%s".*?</svg>' % (slug, slug), re.S)
    n = len(pat.findall(idx))
    if n == 0:
        print('  (not inlined, skipped) %s' % name)
        continue
    if n != 1:
        raise SystemExit('%s is inlined %d times in index.html' % (name, n))
    idx = pat.sub(lambda _m: svg, idx, count=1)
    print('inlined', name)
io.open(idx_path, 'w', encoding='utf-8', newline='\n').write(idx)

# An inlined copy that kept its media query is the bug this step exists to
# prevent, so fail loudly rather than shipping a diagram that ignores the toggle.
for m in re.finditer(r'<svg .*?</svg>', idx, re.S):
    if 'prefers-color-scheme' in m.group(0):
        raise SystemExit('an inlined diagram still carries a dark media query')
print('index.html: all inlined diagrams follow the page theme')
PY

# Credits

Material in this repository that uze did not author, and the terms it comes
under. `NOTICE` points here; the trademark statement on the site and in the
README says the same thing about the marks themselves.

Every entry below is artwork used to identify a harness uze delivers to.
Nothing here is executable, and nothing here is modified beyond the recolour
each entry records.

## Harness marks

| File | Source | Licence of the file | Modification |
| --- | --- | --- | --- |
| `web/public/harnesses/claude-code.svg` | [simple-icons](https://github.com/simple-icons/simple-icons), `icons/claudecode.svg` | CC0-1.0 | `fill="currentColor"` added so it takes the page's theme |
| `web/public/harnesses/opencode.svg` | [simple-icons](https://github.com/simple-icons/simple-icons), `icons/opencode.svg` | CC0-1.0 | `fill="currentColor"` added so it takes the page's theme |
| `web/public/harnesses/codex.png` | OpenAI's own favicon, 128×128, fetched from the vendor's site | No licence granted — see below | none |
| `web/public/harnesses/antigravity.png` | Google Antigravity's own favicon, 180×180, fetched from the vendor's site | No licence granted — see below | none |

The two path strings also appear inline in `web/app/(home)/page.tsx`, which
draws them as SVG rather than embedding a document: a second copy of the same
artwork, under the same terms. Everywhere else the four files are referenced
by URL — including `IntegrationPort::icon_path`, which each integration
answers with the public path of its own mark.

**simple-icons is CC0-1.0, and its own disclaimer is the part that matters
here:** the dedication covers the project's icon files, not the brands they
depict. A CC0 file of someone's logo is still their logo.

**The two favicons carry no licence grant at all.** They are used at their
real brand colours to identify a product uze integrates with — nominative
use, the same basis on which the names are used throughout — and the vendors
retain every right in them. Neither vendor has reviewed, approved or endorsed
this project. If either objects, the honest fix is to drop the mark, not to
argue the point: `IntegrationPort::icon_path` returning `None` renders the
name alone, and the site handles that case already.

## What this file does not cover

Dependencies. The published `uze` binary statically links its Rust
dependencies and the site is built from npm packages; each keeps its own
licence, and no inventory of them is generated today. The site's typefaces
(IBM Plex Sans and IBM Plex Mono, OFL-1.1) are fetched at build time by
`next/font/google` and are not committed here.

uze's own recordings — `web/public/uze-demo.gif` and its poster frame — are
first-party, produced from the spec in `.demo/`, and are covered by this
project's own licence.

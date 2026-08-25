# The design system

This document is byte-identical in the lockbox, hearth, and den
repositories. That is the point of it: these programs are made by the same
hand, and a shared description is cheaper to keep true than descriptions
that agree by accident.

| | |
| --- | --- |
| **lockbox** | a tamper-evident record of how a document was written |
| **hearth** | a local-first assistant that can use your machine |
| **den** | a launcher for the games you already own |
| **lanflix** | a media server for your own network |

Different jobs. One interface.

Lanflix wears the system on a different stack (React and Tailwind), so
instead of another copy of this file its repository carries an adaptation
note, `docs/design.md`, that defers to this document and records only
where the stack forces a different mechanism. Den carries this document
and adds its own short adaptation note, `docs/design-notes.md`, for the
places a launcher differs.

---

## 1. Greyscale, by rule

There is no accent colour, in any of the programs, anywhere.

**Colour never encodes status.** Status is always a word. Every state in
these interfaces must survive being printed in grey, screenshotted into a
bug report, and read by the one person in twelve who does not see the
difference between the green one and the red one — because in all three
cases the colour was never the thing carrying the meaning.

What colour would have done, depth does instead:

| Depth | Means | Example |
| --- | --- | --- |
| **Raised** | interactive | buttons, toolbars, cards, the lock screen panel |
| **Sunken** | where you type, or where content is quoted back | inputs, code blocks, a status readout |
| **Flat** | where you read | the document, the message column |

Emphasis is **inversion**, never a tint: the confirming button in a dialog
is solid foreground with background-coloured text. A destructive action is
marked by its verb — "Permanently delete" — and by whatever gate stands in
front of it, not by red.

### The exceptions

Lockbox tints syntax highlighting and the two sides of a diff (`--syn-*` in
`tokens.css`). That is not a judgement about anything; it is a label for
what a run of characters *is*, in the one place a reader is scanning
structure rather than reading prose. Those seeds are held to three rules:
desaturated enough to sit under text for hours, 4.5:1 against their surface
in both modes, and **nothing depends on them** — every token also carries
weight or slant, so code stays legible printed in grey.

Lanflix keeps colour in poster art, episode stills and the video itself,
because those are the things being kept and watched rather than the
interface. Den keeps it in box art on the shelf for the same reason. Chrome
that floats over artwork — player controls, a hover title, a progress bar
across a still — is white-on-scrim in both modes: it sits on the content,
not on the interface.

Hearth has no such case and therefore has no such exception.

---

## 2. Tokens

`tokens.css` in lockbox, hearth, and den, `app/src/index.css` in lanflix,
and it is the only stylesheet the application chrome reads that holds a
colour literal. Standalone pages a program serves or exports carry their
own embedded copy of the palette, because nothing guarantees them the
token file — hearth's remote client, lockbox's verifier and portal — and
what they embed is the same greys. (Lockbox's whiteboard palette and its
print-only recovery sheet are its recorded exceptions; its own notes say
where and why.) Two layers:

- **Seeds** — the raw palette, one flat list of literals per mode. Every
  value is a literal rather than something derived with `color-mix()`,
  because these programs ship inside a WebView and a palette that silently
  collapses to nothing on an older one is worse than a longer list.
- **Semantic aliases** — `--surface`, `--text-1`, `--edge`, `--ring`, and
  the rest. Components read only these, so a seed edit reaches every
  surface without any stylesheet knowing about a component. (Lanflix
  additionally maps the aliases onto Tailwind utility names; the aliases
  stay the source of truth.)

The seeds are the same values in all four programs. Light and dark are
both defined at plain `:root` specificity under a media query, and again
under `:root[data-theme="…"]` so an explicit choice outranks the system.

Depth is six shadows and a seam (`--lift-1` … `--lift-4`, `--press-1`,
`--press-2`, `--seam`), built from exactly two colours: `--shade`, where a
surface falls away from the light, and `--glow`, where it catches it. The
glow is kept faint and its offsets shallow — enough to say which surfaces
are raised and which are sunken, never enough to read as moulded plastic.

Motion collapses to instant under `prefers-reduced-motion`, once, in the
token file, rather than being fought with `!important` at every site that
animates.

The lock screen card's entrance is `rise` over `--dur-4`, in each program
that has one: the one entrance given the longest duration, because it is
the first thing seen and it plays once.

---

## 3. Type

| Role | Face | Where |
| --- | --- | --- |
| `--font-display` | Gilroy | brand, headings, dialog titles |
| `--font-ui` | Avenir Next LT Pro | everything else |
| `--font-mono` | system mono stack | code, paths, commands |
| `--font-serif` | Georgia and system serifs | lockbox's authoring preview only; the others do not use it (app-specific, like `--measure`) |

Both faces are bundled, not hoped for. Avenir ships Regular and Bold only,
so `--w-normal` and `--w-bold` are the only weights any program asks
for: anything between them gets a synthesized weight that looks smudged at
small sizes.

Lockbox additionally bundles three handwriting faces for its whiteboard
(`--font-hand`, `--font-script`, `--font-print`), under the SIL Open Font
License. A drawing that looks handwritten on the machine it was made on and
typeset on the next one is not a drawing anybody trusts. The others bundle
no such faces and carry no `@font-face` rules asking for them.

---

## 4. Icons

One set, one geometry: a 24×24 box, stroked rather than filled, weight set
in CSS so a glyph stays a hairline at any zoom.

- lockbox: `apps/desktop/src/ui/icons.ts`
- hearth: `src/js/icons.js`
- den: `apps/desktop/src/ui/icons.ts`
- lanflix: lucide-react, reweighted once in CSS to the same stroke

A glyph that appears in more than one hand-rolled set is the same glyph,
character for character. Each set builds every icon with `createElementNS`
from a table of path data — never as an HTML string — so a glyph is safe
to hand a caller that got its name from a plugin: the icon path never
touches `innerHTML`. Lanflix, built on React, takes the stock lucide set
its stack ships rather than adding a fourth table to keep in sync; the
geometry is the same, and the weight still lives in one CSS declaration.

A handful of emblems are filled rather than stroked: a keyhole, a flame, a
play triangle, a status pip. Those are shapes that close up into a blob at
16px, and they are emblems rather than outlines.

Hearth additionally carries an alias table from the Bootstrap Icons names
it used to draw, because that markup is spread across a hundred files *and*
stored in user data — a saved persona record carries its icon name. New
markup there should use `data-icon="…"` and this set's own names.

---

## 5. The marks

`tools/brand.py`, byte-identical in the lockbox, hearth, and den
repositories, holds their marks as SVG path data on the same 24×24 grid at
the same 2.0 stroke, and rasterizes them onto the application icons.

Each is **a container holding one emblem**, because that is what each
program is:

| | | |
| --- | --- | --- |
| **lockbox** | a box, closed, with a keyhole | a place things are kept |
| **hearth** | an arch, open, with a fire | a place things are done |
| **den** | a CRT, with a play triangle | a place games are played |
| **lanflix** | a screen, open, with a play triangle | a place things are watched |

The mark on the lock screen is the same path data as the mark in the dock —
not a picture that resembles it. `tools/check_brand.py` compares them path
by path, re-renders the committed icon files, and fails on either kind of
drift. All three repositories run it in CI.

To change a mark: edit `tools/brand.py`, copy it to the sibling
repositories, run `tools/generate_icons.py` in each, and commit what it
writes.

Lanflix keeps the same rule with its own generator: its mark's path data
lives in `app/public/logo.svg`, the in-app `BrandMark` component and
`scripts/gen-icons.mjs`, which rasterizes the application icons in the
palette's own two greys. Changing that mark means editing all three and
committing what the script writes.

The renderer is pure standard library — PNG is a signature, three chunks
and a CRC; ICO and ICNS are containers around those PNGs; the path
rasterizer is a few hundred lines. An icon is the one asset a reader cannot
diff, and a repository that ships binaries nobody can regenerate
accumulates files whose provenance is a shrug.

---

## 6. Dialogs and the lock screen

The **lock screen** is the first surface anyone sees, so it is the design
in miniature: a flat canvas, one raised card floating on it, the brand mark
on its own raised tile above the name, and a sunken well to type into. No
gradient behind the card — a radial wash bands visibly on panels that are
not perfectly calibrated, and the card's own shadow was already doing that
job. (In lanflix, which has nothing to lock, the first-run setup screen
plays this role. In den, which keeps only what is already on the machine,
the library itself is the first surface, and an empty one says so in
words.)

A **dialog** is the same material as everything else: the raised surface,
the same radius, the same shadow. Not a darker panel pasted on top. One
module owns dialog behaviour in each program that has them (`ui/dialog.ts`,
`js/dialog.js`) so that no call site decides what a dialog looks like.

---

## 7. Accessibility

All four programs commit to WCAG 2.2 AA. Lockbox enforces the mechanical
share of it in CI (`npm run a11y`, axe against the built frontend), which
is how the too-quiet `--text-3` grey was caught — it looked like a
hierarchy and read at 2.5:1.

The greyscale rule is doing accessibility work as a side effect: an
interface with no colour-carried state cannot fail the colour-alone
criterion. That is a reason to keep the rule, not a reason to stop
checking.

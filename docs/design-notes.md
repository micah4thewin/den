# Den and the shared design system

[`DESIGN.md`](DESIGN.md) beside this file is the family document, byte-identical
in the lockbox, hearth, and den repositories. It describes the system and names
all four programs; this note is Den's adaptation, in the same spirit as
lanflix's — only the places where a launcher differs.

Den is a fourth program on the same interface: greyscale by rule, three
materials, one icon geometry, a container holding one emblem.

## The mark

**Den** is a CRT with a play triangle in it — a container holding one emblem,
like its siblings: a place games are played. Its path data lives in `tools/brand.py` alongside its
siblings and is drawn in the interface by `brandDen` in
`apps/desktop/src/ui/icons.ts`. `tools/check_brand.py` compares the two path
by path, re-renders the committed icons, and fails CI on either kind of drift.
It is the same drawing in both places, not two that resemble each other.

## Where Den differs from the document

- **No syntax exception.** The `--syn-*` seeds are carried in `tokens.css`
  because the seed list is shared, but Den displays no code and no diffs, so
  nothing reads them. Like hearth, Den has no case for the exception.

- **No handwriting faces.** Lockbox bundles Caveat, Architects Daughter, and
  Patrick Hand for its whiteboard. Den has no whiteboard, so it bundles
  neither the files nor the `@font-face` rules that would ask for them — an
  `@font-face` pointing at a file the repository does not ship is six requests
  that can only fail. Den ships Gilroy and Avenir Next LT Pro and asks for
  nothing else.

- **No lock screen.** Den keeps a library of files that are already on the
  machine; there is nothing to unlock. The first surface is the library
  itself, and an empty one says so in words rather than showing a spinner.

- **Box art is the exception colour is allowed.** Like lanflix's posters, the
  artwork on a tile is the thing being kept, not part of the interface. The
  chrome around it has no accent colour anywhere, which is why a tile with no
  art still reads: the title sits on the quiet tile in its place.

## Status is a word, in this program too

The intake report card is the clearest case in Den: eight outcomes, each one a
word in a raised pill, each one legible printed in grey. `quarantined` is not
red and `added` is not green. The tally above the list is the same eight words
counted. Nothing on that screen would lose meaning in a black-and-white
screenshot pasted into a bug report, which is the whole test.

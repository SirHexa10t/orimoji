# Ideas

What orimoji doesn't do yet, and what each would buy. The numbers behind them are in
[observations.md](observations.md).

- **Tables for tiny files.** On arterminal's 1.1 KB braille drawing, the text squeeze's code table
  outweighs the body: 63% of the drawing, where crumple gets 43%. Code points written as small
  steps from the one before, as crumple writes its table, would bring it near crumple's.
- **Deciding a shared index together.** Files folded together share one index, but each still
  decides alone what is worth listing, as if it paid for the whole list itself. Deciding together,
  a value several files use would pay its line once, and more of them would make the list.
- **A smaller index.** The index is kept as plain text, a value a line. A caller who wants it small
  can fold it with orimoji too; orimoji could offer that as a form of its own.
- **Context.** Every symbol is coded on its own. Coding a tag by the tag before it, or a character
  by the one before, would shrink the listing's tag lists and names further.
- **The row above**, for drawings: in crumple, and as a text squeeze that leans on it.
- **brotli and xz.** Over a whole drawing they came within a few percent of zstd 19, at 1 to
  6 MiB/s; zstd is here instead.
- **One alphabet of one- and two-byte characters.** In theory it reaches 81% of base64's
  characters at 90% of its bytes, but it needs arithmetic coding, and in git its gain would likely
  vanish, as base94's does.

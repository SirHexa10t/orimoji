# Ideas

What orimoji doesn't do yet, and what each would buy. The numbers behind them are in
[observations.md](observations.md).

- **Folded files as small as parts.** A fold into parts has a compact table, runs only where they
  pay, and counts in a code chosen for them; a folded file still writes a line per symbol, deflated,
  every three alike as a run, and counts in Elias gamma. On arterminal's drawings the parts take
  3,787, 413 and 566 B for the characters, a folded file of them 4,084, 698 and 738. A folded file
  could name the compact table in its header, and the choices with it.
- **An index given as a key.** A caller that already holds the values worth indexing could hand them
  to the fold, instead of orimoji choosing an index and the caller keeping it. arterminal needed it
  while orimoji folded a drawing's colours with its characters: Miku's index was 153 B of escapes
  her palette already lists. Folding the characters alone, and keeping the colours as palette
  numbers itself, it no longer does; a caller folding text full of values it keeps elsewhere anyway
  still would.
- **Code lengths in a code of their own.** A compact table gives every code length five bits. A
  braille drawing's lengths run from 3 to 8, and in a Huffman code of their own, as deflate codes
  its code lengths, Kirino's would take 188 bits instead of 400: 35 B of her 443. The long art would
  save 5 B, Miku none.
- **Pairs and triples of characters, a layer for later.** Words and terminal escapes are single
  symbols in an indexed fold already; pairs and triples of characters, words without a word's
  boundaries, could be too, wherever one repeats enough to pay its place in the table. Terminal art
  drawn in full blocks needs it most: it makes a pixel of two characters, a cell being twice as tall
  as it is wide, so every pixel is spelled twice and every run counted in characters. All of Miku's
  runs are of even length, where the long art and Kirino have no row of pairs at all. Any pair of
  one character taken as a unit, wherever it falls, takes 28% off Miku's character rows but adds to
  the long art's, whose characters pair by chance, so it would be chosen by folding both ways and
  keeping the smaller. A narrower form needs no such choice: a line made of pairs throughout starts
  with a symbol saying so and is spelled a pair to a symbol, which takes 24% off Miku's rows and
  costs a text with no such line nothing, the symbol not even in its table; a scan could find pixels
  three wide too. Either is a layer over the text squeeze, to add once the rest stands: units are
  chosen before the code is built, as words are.
- **`.ori`: parts in a file of the command line's own.** The command line, one more caller of parts,
  could keep them in a file of its own (`--into-file`), the index and the choices inside, so that it
  unfolds alone. Files folded together would each hold a copy of their shared index and choices, or
  keep the index once, beside them, with `--index FILE`.
- **A bound on what a folded file unfolds to.** Parts unfold within a limit their caller gives,
  refused before more is made; a folded file doesn't, and one made to do harm can stand for far more
  than its caller would ever accept. The same limit, checked as each line is read, would refuse it;
  only the whole-file folds stop now, at their `size=`.
- **Deciding a shared index together.** Files folded together share one index, but each still
  decides alone what is worth listing, as if it paid for the whole list itself. Deciding together,
  a value several files use would pay its line once, and more of them would make the list.
- **Measuring list by list.** A measured fold weighs the whole index against none. Trying lists one
  at a time (a column's, the spelling's) could keep the ones that pay and drop the rest.
- **Stopping a losing fold early.** A fold weighed against a smaller one could stop as soon as its
  text outgrows it: the measurement without the index on the listing, or `auto`'s `pack`, which
  makes 66 MB to lose to the table's 12.5 MB. The answer would be the same, sooner, though the
  loser's size would no longer be known.
- **`auto`, measured every way.** Measuring, `auto` weighs the squeeze it chose without its index;
  it could weigh every candidate both ways instead, for about twice the time.
- **A smaller working set.** At its peak a fold takes 4 to 7 times its input, most of it the
  squeeze's models and the squeezed lines; the lines could be folded and freed one at a time.
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

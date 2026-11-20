# Ideas

What orimoji doesn't do yet, and what each would buy, in the order they are to be done. The
numbers behind them are in [observations.md](observations.md).

## Next

- **Units for the other squeezes.** The unit rule is built for the text squeeze: a fold's choices,
  code table and index are kept together, inside a file folded alone (`.ori`), or once in an
  `.orinv` for files folded together, which name it and its fingerprint (see the README's
  [Files folded together](../README.md#files-folded-together) and
  [format.md](format.md#ori-and-orinv)). The other squeezes keep their index inside each folded file
  (format 2), and fold files each on its own, each with tables of its own. The table squeeze's
  column models, learnt over files folded together and kept once, would be its unit.
- **Several groups.** Files given together share one unit, and each that takes less alone folds
  alone (see [observations.md](observations.md#files-folded-together)). Files unlike the rest but
  like each other, two books or several drawings in one style, each fold alone so, where they could
  share a second unit: a second `.orinv`. The set would be split into groups, each weighed as the
  one group is now.
- **Grouping in less time.** Grouping folds every file alone as well as all of them together, and
  together again for each set it tries: two to three times what folding them together takes. The
  estimate needs only each file's counts, and the code learnt over the set; a fold alone is needed
  only for the files the estimate says might leave.
- **The first format's way.** `Fold::index_inside` off, and `orimoji::fold_all` given several
  inputs, still fold as the first two formats did: the text squeeze into a folded file, and an
  index shared by texts that each have a code of their own, kept beside them or inside each.
  Nothing else writes so now, and folds of those formats still unfold. Kept for now.
- **Stopping a losing fold early.** A fold weighed against a smaller one could stop as soon as its
  text outgrows it: the measurement without the index on the listing, or `auto`'s `pack`, which
  makes 66 MB to lose to the table's 12.5 MB. The answer would be the same, sooner, though the
  loser's size would no longer be known.
- **A smaller working set.** At its peak a fold takes 4 to 7 times its input, most of it the
  squeeze's models and the squeezed lines; the lines could be folded and freed one at a time.
- **A smaller index.** The index is kept as plain text, a value a line: as it is in a unit, deflated
  with the tables in a folded file. Genesis's 507 words take 3,411 of its `.orinv`'s 3,872 bytes;
  deflated, 1,671. A unit could hold its index in a compact form of its own, its choices saying
  so.
- **Context.** Every symbol is coded on its own. Coding a tag by the tag before it, or a character
  by the one before, would shrink the listing's tag lists and names further. It costs a table for
  every context, which small texts can't afford, so it would be taken only where it pays, the
  choices saying so, as for every other way a fold decides.

## Later

- **A second layer, which needn't be colours.** arterminal folds a drawing as two planes, its
  characters and their colours, each a line a row and each squeezed its own way. Other text has a
  second layer too: Hebrew with its nikud, the vowel points written in and around the letters as
  combining marks, is letters and the marks on them, and the marks are few and follow patterns of
  their own. Split into the two, each folded its own way, it could fold smaller than spelled
  together. A Hebrew Genesis with nikud is to come as the test for it, beside the English one, once
  the rest is done.
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
- **A more methodical code for code lengths.** A compact table writes its code lengths five bits
  each, or in a Huffman code of their own (`src/squeeze/lengths.rs`), each coded on its own. A
  length could be told from what it belongs to, a character or a word, and how common its kind is. A
  better way joins the two as a third, numbered in the choices.
- **Folded files as small as parts.** A fold into parts (and so an `.ori`) has a compact table, runs
  only where they pay, and counts in a code chosen for them; a folded file still writes a line per
  symbol, deflated, every three alike as a run, and counts in Elias gamma. On arterminal's drawings
  the parts take 3,783, 379 and 567 B for the characters, a folded file of them 4,084, 698 and 738.
  A folded file could name the compact table in its header, and the choices with it.
- **A bound on what a folded file unfolds to.** Parts, and `.ori` files in the library
  (`ori::unfold`), unfold within a limit their caller gives, refused before more is made; a folded
  file doesn't, nor does the command line give an `.ori` one, and a file made to do harm can stand
  for far more than its caller would ever accept. The same limit, checked as each line is read,
  would refuse it; only the whole-file folds stop now, at their `size=`.
- **An index given as a key.** A caller that already holds the values worth indexing could hand them
  to the fold, instead of orimoji choosing an index and the caller keeping it. arterminal needed it
  while orimoji folded a drawing's colours with its characters: Miku's index was 153 B of escapes
  her palette already lists. Folding the characters alone, and keeping the colours as palette
  numbers itself, it no longer does. Against keeping the index with the code (the first entry), the
  unit would name the key rather than hold it. Folding one file, the key is as safe as the unit,
  kept by the same caller; files folded together would take it as a file, as they take their
  `.orinv`, and so would the command line, which never takes a unit or a key as text.
- **Measuring list by list.** A measured fold weighs the whole index against none. Trying lists one
  at a time (a column's, the spelling's) could keep the ones that pay and drop the rest.
- **`auto`, measured every way.** Measuring, `auto` weighs the squeeze it chose without its index;
  it could weigh every candidate both ways instead, for about twice the time.

## At the end

- **brotli and xz.** Over a whole drawing they came within a few percent of zstd 19, at 1 to
  6 MiB/s; zstd is here instead.
- **One alphabet of one- and two-byte characters.** In theory it reaches 81% of base64's
  characters at 90% of its bytes, but it needs arithmetic coding, and in git its gain would likely
  vanish, as base94's does.

## Decided against

- **The row above.** Coding a line by the line above it is most of what a whole-file compressor gets
  that a line-by-line fold doesn't. But then one line changed changes the lines below it too, and
  git stores them all again: smaller files, and a larger repository. orimoji stays a per-line
  worker, in the context of the whole file.

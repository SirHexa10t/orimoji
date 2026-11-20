# orimoji - folded characters

Fold text, or any data, into as few characters, as few bytes or as narrow a line as the job needs,
and unfold it back exactly. *Ori* (折り) is the fold of origami; *moji* (文字), the character of emoji.

```sh
orimoji fold --index listing.tsv            # listing.tsv.folded, its index inside it
orimoji unfold listing.tsv.folded           # listing.tsv again
orimoji fold -s text --index chapters/*.txt # an .ori each, their unit in chapters/chapters.orinv
orimoji unfold chapters/*.ori               # the chapters again
orimoji fold --in-place drawings/*.txt      # the files themselves, folded
orimoji fold -s zstd -a cjk < notes.txt     # standard input to standard output, in CJK
orimoji measure drawing.txt                 # every way, compared, git included
orimoji alphabets
```

## What it does

A fold is two steps:

- **Squeeze** makes the data smaller.
- **Fold** writes what is left as characters of an alphabet. The alphabet decides what each bit
  costs in characters, in bytes and in screen width.

The folded text names both in its first line and keeps the rest of what it needs, its code tables
and its index, so **unfold** gives back the original, byte for byte, from it alone. Text files
folded together keep what they share once, in a file beside them (see
[Files folded together](#files-folded-together)). orimoji folds a string, standard input, or files:
the files themselves, or new files beside them under a suffix (`.ori` for the text squeeze,
`.folded` for the others), as the caller chooses.

| Squeeze | What it does | A line per line |
|---|---|---|
| `table` | Rows of cells, tab-separated by default. Each column gets the model that costs it least: numbers, distances from the row above (for sorted ids), lists of items (`,` or `;`), or spelling; with `--index`, a list of its common values too. Comment lines (`#`) stay readable. | yes |
| `text` | Every line spelled in one Huffman code the whole file shares, with runs as single symbols; with `--index`, common words and terminal escapes too. | yes |
| `pack` | Each character as its place among the characters the file uses, in just the bits that many need: for text of a small set, like digits. | yes |
| `deflate` | The whole file with `--whole`; otherwise a line at a time, each leaning on the `--lean` lines above. | without `--whole` |
| `zstd` | The whole file. | no |
| `none` | The bytes as they are. | without `--whole` |

The alphabets run from ASCII ones, a byte a character, to wider ones that hold more bits in a
character but spend more bytes on it:

- **ASCII:** `hex`, `base32`, `crockford`, `base58`, `base64url` (the default), `z85`, `base91`,
  `base94`.
- **Past ASCII:** `braille`, `two-byte`, `one-cell`, `cjk`, `cjk-ext-b`: fewer characters and
  narrower lines, never fewer bytes.

`orimoji alphabets` lists them with what a character of each holds and costs.

### A line per line

A line squeeze folds every line of the original into a line of its own, so a folded file kept in a
repository diffs the way the original would. Its tables (codes, lists of values) come from the
whole file, so folding a new version afresh can change every line; `--like old.folded` (or
`old.ori`) folds it with the old file's tables instead, and the lines that didn't change don't
change.

### Indexes

Asked to, the table and text squeezes write values as their places in a list: a column's common
values, or a text's common words and terminal escapes. A list must pay for itself twice over, or it
is left out. Indexing is off by default, since a fold without one is simpler to follow; on, it is
usually much smaller.

The index goes with the code it is written in, and is never parted from it: inside the folded
file, or, for text files folded together, in the `.orinv` they share. A fold that was asked to
index says what became of it: no index, because nothing paid for itself; or the index, kept with
the code. Folds of orimoji's first format kept the index beside the text, in a file of their own;
they still unfold, given it with `--index FILE`, and refuse to without it, or with another.

The text squeeze folds both with the index and without, and keeps whichever is smaller, the index
counted. For the table squeeze, whether an index pays is estimated. To know instead, give the fold
a time limit (`--measure-up-to 5s`): it also folds without the index, keeps whichever fold is
smaller, index included, and says which. The fold kept is the one already made, not folded again.
If measuring takes longer than the limit, it gives up and the estimates decide. While the second
fold is made, the first waits in a temporary file once it is past 8 MiB, so the two are never in
memory together. The file goes beside the output, or into `TMPDIR` for standard output.

### Files folded together

Text files given together (`-s text`, or `auto` where it chooses the text squeeze) fold together
where that takes less. Those that share one unit, learnt over them (the choices the fold made, the
code table their lines are spelled in, and the index), keep it once, in an `.orinv`; each one's
`.ori` holds its lines, and names the `.orinv` by a path from its own folder and the fingerprint of
the unit it holds, so it is never unfolded with another. The `.orinv` goes beside the first file
that shares it, named after its folder (`chapters/chapters.orinv`), or into `--orinv-dir`. Each
`.ori` then unfolds alone, with the `.orinv` where it names it, or one given with `--orinv FILE`.

A file unlike the rest folds alone instead, as if it had been given alone, its unit inside its
`.ori`: sharing a code, its lines would be spelled in one that suits them worse, and its symbols
would crowd everyone else's, for less than the unit it would save. Which files those are is
estimated first, each weighed against the others (the set's counts less its own), and those that
would save by leaving try it, the rest folded together again. Then the folds decide: what is
written is whichever takes least of every file alone, all of them together, and each set tried.
The fold says how many share. One file folds alone; to fold files each alone, fold each on its
own.

| Files given together | All together | Each alone | As folded |
|---|--:|--:|--:|
| Genesis's 50 chapters | 124,055 | 152,442 | 124,055, all sharing |
| Three of the example drawings | 6,158 | 5,449 | 5,449, each alone |
| Eight chapters and two drawings | 24,687 | 24,511 | 22,438, the chapters sharing |

Bytes, the `.orinv` included. Weighing takes two to three times what folding them together does:
69 ms for Genesis, against 30. `cargo run --release --example folded_together -- FILE…` shows the
three on any files.

An `.orinv` other files still unfold with is never replaced by another unit: such a fold is refused,
and names those files. A file joins them by folding `--like` their `.orinv` (or one of their `.ori`
files), keeping their unit, so the `.orinv` doesn't change.

Files of the other squeezes fold each on its own, its index inside it.

## Using it

### The command line

| Command | Does |
|---|---|
| `orimoji fold [FILE…]` | folds standard input onto standard output (or `-o FILE`), or files beside themselves, or over themselves |
| `orimoji unfold [FILE…]` | gives back what folded files hold, the same ways |
| `orimoji measure [FILE]` | folds it every way and compares characters, bytes, width, and what git stores |
| `orimoji alphabets` | lists the alphabets, and what a character of each costs |

`unfold` takes `--orinv FILE` (the `.orinv` of `.ori` files, when it isn't where they name it),
`--index FILE` (the index of a first-format fold, which kept it beside the text), `--in-place`,
`--suffix` and `--output` too. `fold` takes:

| Option | Does |
|---|---|
| `-s`, `--squeeze` | `auto` (the default: folds with `table`, `text` (into an `.ori`), `pack` and line-by-line `deflate`, and keeps the smallest; text files given together fold together where that takes less), `none`, `pack`, `text`, `table`, `deflate` or `zstd` |
| `-a`, `--alphabet` | any of the alphabets; `base64url` by default |
| `--index` | lets `table` and `text` index, where a list pays for itself; the index is kept with the code |
| `--measure-up-to TIME` | with `--index`: measures, rather than estimates, whether the index pays, for TIME at most (`30ms`, `5s`, `15m`, `1h`, `2d`); the text squeeze always weighs both |
| `--in-place` | writes over the files instead of beside them |
| `--suffix S` | the suffix of the files written beside them (`.ori` for `text`, `.folded` for the others): a fold adds it, an unfold takes it off |
| `-o`, `--output FILE` | writes there instead, for standard input or a single file (`-` for standard output) |
| `--like FOLDED` | keeps the tables and index of an earlier fold; for `text`, the unit of an `.ori` or an `.orinv`, joining the files that share it |
| `--orinv-dir DIR` | puts the `.orinv` of text files folded together in DIR |
| `--whole` | squeezes the whole file as one stream (`none`, `deflate`) |
| `--lean N` | leans each line of a line-by-line `deflate` on the N lines above (1) |
| `--separator C`, `--keep PREFIX` | split `table` cells on C (`tab`), and keep lines starting with PREFIX as they are (`#`) |
| `--readable` | keeps the tables readable text instead of deflating them |
| `--wrap N`, `--level N` | set a whole-file body's line width in cells (100), and zstd's level (19) |
| `-q`, `--quiet` | says nothing on standard error |

### Choosing

- **Kept in a repository, diffed line by line:** `table` for columns, `text` for other text, both
  with an index, into `base64url`, each new version folded `--like` the last; text files that
  spell alike, folded together.
- **Smallest file, no diffs needed:** `zstd`, or `deflate --whole` in a build without zstd.
- **Narrowest on screen:** `two-byte`; `one-cell` is narrower still, where fonts cover it.
- **Fewest characters, where a limit counts them:** `cjk` or `cjk-ext-b`, two cells wide each.
- **Typed or read out by people:** `crockford`, which reads back what people type, or `hex`.
- **No lookalikes, and a double-click selects all of it:** `base58`.
- **Unquoted in URLs, file names, JSON and shell words:** `base64url`, `base32` or `hex`.
- **Text from a small set, like digits or hex:** `pack`.
- **Showing its bits:** `braille`, which draws each byte as its dots.

`orimoji measure` shows which wins on a given file.

### In code

A library first: the binary is a thin shell over the crate.

```rust
use orimoji::{Alphabet, Fold, Squeeze, TableOptions};

let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url).indexed();
let folded = orimoji::fold(&std::fs::read("listing.tsv")?, &how)?; // its index inside it
let newer = std::fs::read("listing-next.tsv")?;
// Unchanged rows fold to unchanged lines.
let next = orimoji::fold_keeping(&newer, &folded.text, None, &how)?;
assert_eq!(orimoji::unfold(&next, None)?, newer);
```

`orimoji::files` folds and unfolds files exactly as the command line does: in place or beside
themselves (`Output::InPlace`, `Output::Suffix`), text files together into `.ori` files and an
`.orinv` where that takes less (`orimoji::ori` reads and writes those), the `.orinv` where
`Fold::orinv_dir` says. `orimoji::fold` folds the text squeeze into an `.ori` too, as `auto` does
where it chooses it.
`orimoji::fold_all` folds several inputs with one index learnt over them all. `Squeeze::Auto` is
`auto`, and every fold says which squeeze made it (`folded.squeeze`). `Fold::measured_up_to`
measures whether the index pays, `Judged` says how it was decided, and `orimoji::time_limit` reads
limits like `5s`.
`Fold::buffer_past` and `Fold::buffer_folder` say when and where a fold waiting to be weighed goes
to a temporary file. `Fold::index_inside` off folds as the first format did, the index left out of
the folded text for the caller to keep; `Index::to_text` and `Index::from_text` keep an index as
text, and `orimoji::needs_index` says whether a stored text needs one. `orimoji::measure` measures,
`orimoji::words` writes words from a fixed list as numbers, and `Alphabet` folds bytes or bits by
itself.

### Parts

A program with a file format of its own folds into parts instead (`orimoji::parts`), and gets
values back, not a file: a line for each line of the text, and their unit, which always goes
together: the choices the fold made and the code the lines are spelled in, each one line of the
alphabet, and the index when one paid. The program keeps them where it suits, under no header of
orimoji's, and gives them back to unfold; arterminal keeps a drawing's characters so.
`parts::fold_together` folds several texts into one unit, `parts::fold_grouped` only those that
take less so, each of the rest alone, and `Unit::to_text` and `Unit::from_text` keep a unit as
text.

```rust
use orimoji::{Alphabet, parts};

let folded = parts::fold("⣿⣿⣿⡇\n⣿⡇ ⢸", Alphabet::Base64Url, false);
// folded.lines, and folded.unit (its choices, table and index), kept wherever suits:
let back = parts::unfold(&folded.lines, &folded.unit, Alphabet::Base64Url, 1 << 20)?;
```

It is the text squeeze, with what a folded file doesn't have yet: a compact code table, each
character a step from the one before, its code lengths five bits each or in a code of their own;
a run only where it is shorter than writing its unit again; and runs' counts in whichever code
writes them shortest. Wherever the fold could write something more than one way, it weighs them
and takes the shortest, and the choices record each decision (`parts::decisions` reads them). The
choices also carry a fingerprint of the table and the index, so parts that don't belong together
are refused rather than read wrong, and unfolding takes a limit, refusing a fold that would come to
more before making it. The format is in [docs/format.md](docs/format.md#parts).

### Examples

```sh
cargo run --example alphabets                  # one sentence in every alphabet
cargo run --example drawing                    # a terminal drawing, and a repainted row
cargo run --release --example steam_listing    # a game catalogue as a table, and a new sweep
cargo run --release --example folded_together # Genesis and two drawings: together, alone, grouped
cargo run --example fixed_words                # weekdays, months and records as numbers
```

### Building

`cargo build --release`. The default features are `cli`, the command line (clap), and `zstd`,
which builds zstd's C library; a program that only needs the library can depend on orimoji with
`default-features = false`.

## Read more

- [docs/theory.md](docs/theory.md): why folding works, and what each choice costs by definition:
  the alphabets in full, the space plain text wastes, words from a fixed list, indexes, lines and
  diffs, and git.
- [docs/format.md](docs/format.md): the folded file, line by line and bit by bit.
- [docs/observations.md](docs/observations.md): what the squeezes and alphabets do to real data,
  how fast, and what git makes of it.
- [docs/ideas.md](docs/ideas.md): what isn't done yet.

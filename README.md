# orimoji - folded characters

Fold text, or any data, into as few characters, as few bytes or as narrow a line as the job needs,
and unfold it back exactly. *Ori* (折り) is the fold of origami; *moji* (文字), the character of emoji.

```sh
orimoji fold --index listing.index listing.tsv  # listing.tsv.folded, and its index beside it
orimoji unfold --index listing.index listing.tsv.folded  # listing.tsv again
orimoji fold --in-place drawings/*.txt          # the files themselves, folded
orimoji fold -s zstd -a cjk < notes.txt         # standard input to standard output, in CJK
orimoji measure drawing.txt                     # every way, compared, git included
orimoji alphabets
```

## What it does

A fold is two steps:

- **Squeeze** makes the data smaller.
- **Fold** writes what is left as characters of an alphabet. The alphabet decides what each bit
  costs in characters, in bytes and in screen width.

The folded text names both in its first line, so **unfold** needs nothing else to give back the
original, byte for byte, unless the fold made an index, which comes back separately (see
[Indexes](#indexes)). orimoji folds a string, standard input, or files: the files themselves, or new
files beside them under a suffix (`.folded`), as the caller chooses.

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
whole file, so folding a new version afresh can change every line; `--like old.folded` folds it
with the old file's tables instead, and the lines that didn't change don't change.

### Indexes

Asked to, the table and text squeezes write values as their places in a list: a column's common
values, or a text's common words and terminal escapes. A list must pay for itself twice over, or it
is left out. Indexing is off by default, since a fold without one is simpler to follow; on, it is
usually much smaller.

The index doesn't go into the folded text: it comes back beside it, for the caller to keep where it
suits (inside a file format of its own, beside the text, or once for many texts), and to give back
when unfolding. Files folded together share one index. A fold that was asked to index says what
became of it: no index, because nothing paid for itself and the text unfolds alone; or the index to
keep. A text that needs an index says so in its first line, and won't unfold without it, or with
another.

## Using it

### The command line

| Command | Does |
|---|---|
| `orimoji fold [FILE…]` | folds standard input onto standard output (or `-o FILE`), or files beside themselves, or over themselves |
| `orimoji unfold [FILE…]` | gives back what folded files hold, the same ways |
| `orimoji measure [FILE]` | folds it every way and compares characters, bytes, width, and what git stores |
| `orimoji alphabets` | lists the alphabets, and what a character of each costs |

`unfold` takes `--index FILE`, `--in-place`, `--suffix` and `--output` too. `fold` takes:

| Option | Does |
|---|---|
| `-s`, `--squeeze` | `auto` (the default: the smallest line squeeze), `none`, `pack`, `text`, `table`, `deflate` or `zstd` |
| `-a`, `--alphabet` | any of the alphabets; `base64url` by default |
| `--index FILE` | lets `table` and `text` index, and keeps the index in FILE, if one paid for itself; files folded together share it |
| `--in-place` | writes over the files instead of beside them |
| `--suffix S` | the suffix of the files written beside them (`.folded`): a fold adds it, an unfold takes it off |
| `-o`, `--output FILE` | writes there instead, for standard input or a single file (`-` for standard output) |
| `--like FOLDED` | keeps the tables of an earlier fold, and the index it was folded with (`--index FILE`) |
| `--whole` | squeezes the whole file as one stream (`none`, `deflate`) |
| `--lean N` | leans each line of a line-by-line `deflate` on the N lines above (1) |
| `--separator C`, `--keep PREFIX` | split `table` cells on C (`tab`), and keep lines starting with PREFIX as they are (`#`) |
| `--readable` | keeps the tables readable text instead of deflating them |
| `--wrap N`, `--level N` | set a whole-file body's line width in cells (100), and zstd's level (19) |
| `-q`, `--quiet` | says nothing on standard error |

### Choosing

- **Kept in a repository, diffed line by line:** `table` for columns, `text` for other text, both
  with an index, into `base64url`, each new version folded `--like` the last.
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
let folded = orimoji::fold(&std::fs::read("listing.tsv")?, &how)?;
let index = folded.indexing.index(); // keep it wherever suits, if there is one
let newer = std::fs::read("listing-next.tsv")?;
// Unchanged rows fold to unchanged lines.
let next = orimoji::fold_keeping(&newer, &folded.text, index, &how)?;
assert_eq!(orimoji::unfold(&next, index)?, newer);
```

`orimoji::fold_all` folds several inputs into one shared index, and `orimoji::files` folds files in
place or beside themselves (`Output::InPlace`, `Output::Suffix`). `Index::to_text` and
`Index::from_text` keep an index as text, and `orimoji::needs_index` says whether a stored text
needs one. `orimoji::pick` chooses the line squeeze the way `auto` does, `orimoji::measure`
measures, `orimoji::words` writes words from a fixed list as numbers, and `Alphabet` folds bytes or
bits by itself.

### Examples

```sh
cargo run --example alphabets                  # one sentence in every alphabet
cargo run --example drawing                    # a terminal drawing, and a repainted row
cargo run --release --example steam_listing    # a game catalogue as a table, and a new sweep
cargo run --example shared_index               # three drawings, one index
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

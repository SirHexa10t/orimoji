# orimoji - folded characters

Fold text, or any data, into as few characters, as few bytes or as narrow a line as the job needs,
and unfold it back exactly. *Ori* (折り) is the fold of origami; *moji* (文字), the character of emoji.

```sh
orimoji fold listing.tsv -o listing.folded       # the smallest fold that keeps a line per line
orimoji unfold listing.folded -o listing.tsv
orimoji fold --like listing.folded listing.tsv -o listing.folded  # a new version, old lines unchanged
orimoji fold -s zstd -a cjk notes.txt            # the smallest whole-file squeeze, in CJK
orimoji measure drawing.txt                      # every way, compared, git included
orimoji alphabets
```

## Squeeze, then fold

- **Squeeze** makes the data smaller: packing, a Huffman code, a model per column, deflate or
  zstd. `auto`, the default, tries the line squeezes and keeps the smallest.
- **Fold** writes the squeezed bits as characters of an alphabet. The alphabet alone decides what
  each bit costs in characters, in bytes and in screen width.
- **Unfold** reads the folded file's first line for how it was folded, and gives back the original
  byte for byte.

| Squeeze | What it does | A line per line |
|---|---|---|
| `table` | Rows of cells, tab-separated by default. Each column gets the model that costs it least: a list of its common values, numbers, distances from the row above (for sorted ids), lists of items (`,` or `;`), or spelling. Comment lines (`#`) stay readable. | yes |
| `text` | Every line spelled in one Huffman code the whole file shares, with common words, terminal escapes and runs as single symbols. | yes |
| `pack` | Each character as its place among the characters the file uses, in just the bits that many need: for text of a small set, like digits. | yes |
| `deflate` | The whole file with `--whole`; otherwise a line at a time, each leaning on the `--lean` lines above (1 by default). | without `--whole` |
| `zstd` | The whole file, at `--level` 19 by default. | no |
| `none` | The bytes as they are. | without `--whole` |

## The folded file

```text
=== orimoji 1 table base64url ===
=== deflated tables ===
rVnNbtzIET5TT8FFgM0uYnmH_2SyDiBLI9lrydZ6tN6ssZcm2TPTGv6MmqRmRkGABMgtx-SQSy55gBxySnLIIU-QvMI-SeorzlAc
…
=== body ===
 # app_id\ttype\tband\tapproval\treviews\treleased\tos\tvr\tearly\tdeck\tcompat\tlang_ui…
…
E9rdLvvZZTzCnmBKx3Taz2O1r_RiZFhG-vvpC74zDxDS3xOmG7FdMs_0qs_2Rpi9e0w
EvquRJweaiS4iRIsjnAlY5nQpVBo1vee18hjH3lSlb-ut1xTphu5YrLP9KrNRqsLoc-LzhAv2A
```

- **The header line** names the format version, the squeeze, the alphabet and the squeeze's
  options: `whole` and `size=<bytes>` for a whole-file squeeze (the size bounds and checks the
  unfold), `lean=<lines>` for deflate a line at a time.
- **The tables** are what a squeeze shares between lines: a table's column models and value lists,
  a text squeeze's code. They are text, deflated and folded in the body's alphabet whenever that
  makes them smaller; `--readable` keeps them as text, to read what a table squeeze chose.
- **The body** has a line for every line of the original under a line squeeze, so a repository
  diffs the folded file the way it would diff the original. A whole-file squeeze writes its stream
  in lines `--wrap` cells wide (100). A body line starting with a space is a line kept as it was,
  escaped: a table's comments, or a line with another number of cells.

### Keeping the tables

A line squeeze learns its tables from the whole file, so folding a new version afresh can rebuild
them and change every line. `--like old.folded` folds the new version with the old file's tables
instead: every line that didn't change folds to the same body line, and the folded file's diff is
the original's. What the old tables lack (a new studio, a new character) is spelled out instead, a
little larger; fold afresh now and then to bring them up to date. Refolding catalogames' 32 MB
listing with its own tables takes half a second and gives the same file byte for byte.

## Results

### catalogames' listing of Steam's catalogue

`data/steam/steam-games.tsv` in the local copy of catalogames: 32,198,699 bytes, 188,759 lines,
20 tab-separated columns.

| Squeeze, into base64url | Folded file | In git | A line per line | Fold / unfold |
|---|--:|--:|---|--:|
| `table` | 38% | 70% | yes | 1.5 s / 0.4 s |
| `text` | 68% | 105% | yes | 1.3 s / 0.4 s |
| `deflate`, leaning on the line above | 95% | 173% | yes | |
| `none` | 133% | 134% | yes | |
| `pack` | 205% | 385% | yes | |
| `deflate --whole` | 53% | 100% | no | |
| `zstd`, level 19 | 38% | 72% | no | 6.6 s / 0.1 s |

Sizes are against the file; *in git* is against what git stores for the file itself (zlib,
12,896,515 bytes). The table squeeze is as small as zstd over the whole file, smaller in git, and
still a line per row. Into `two-byte` letters it is 46% of the file's bytes and 23% of its width.
`pack` is the wrong tool here: the listing uses hundreds of characters, so a place among them
costs more than a byte.

The table squeeze chose, column by column: distances for the sorted `app_id`; item lists for
`tagids` and `feats` (`,`); and for every other column a list of its common values, with the rest
written as numbers for `reviews` and spelled out for the others, names and studios included. On the
400-row excerpt in `examples/data` it chooses differently (`devs` and `pubs` as `;` lists, `name`
only spelled), since what pays depends on how often values repeat.
`cargo run --release --example steam_listing` prints the choice for the excerpt, or for a listing
given as its argument.

### arterminal's drawings

The three examples of arterminal, as arterminal saves them, into base64url:

| Squeeze | Long art, 13 KB | Kirino, 1.1 KB | Miku, 6.4 KB | A line per line |
|---|--:|--:|--:|---|
| arterminal's own crumple | 29% | 43% | 18% | yes |
| `text` | 30% | 63% | 20% | yes |
| `table` | 36% | 68% | 29% | yes |
| `deflate`, leaning on the line above | 38% | 74% | 22% | yes |
| `pack` | 98% | 64% | 58% | yes |
| `deflate --whole` | 25% | 53% | 16% | no |
| `zstd` | 23% | 56% | 15% | no |

The text squeeze comes within a point or two of crumple on the larger drawings. On Kirino, its
code table outweighs the body itself. In git, though, none of these beats the drawing as it
is: a small, repetitive drawing is what zlib compresses best, and the text squeeze's files take
119–144% of what git stores for the drawings themselves (zstd's, 95–128%). Folding a drawing buys
a smaller checkout and fewer characters, not a smaller repository; the listing above, big and
regular, gets both.

## Alphabets to fold into

| Alphabet | Characters | Count | Bits per character | UTF-8 bytes | Bytes wasted | Screen cells | Fold / unfold, MiB/s |
|---|---|--:|--:|--:|--:|--:|--:|
| `hex` | `0-9 a-f` | 16 | 4 | 1 | 50% | 1 | 285 / 142 |
| `base32` | `A-Z 2-7` | 32 | 5 | 1 | 38% | 1 | 314 / 164 |
| `crockford` | `0-9 A-Z` without `I L O U`, read back in either case | 32 | 5 | 1 | 38% | 1 | 316 / 163 |
| `base58` | `1-9 A-Z a-z` without `I O l` | 58 | 5.86 | 1 | 27% | 1 | 45 / 163 ¹ |
| `base64url` | `A-Z a-z 0-9 - _` | 64 | 6 | 1 | 25% | 1 | 380 / 180 |
| `z85` | `0-9 a-z A-Z` and `.-:+=^!/*?&<>()[]{}@%$#` | 85 | 6.4 | 1 | 20% | 1 | 614 / 820 |
| `base91` | `!` to `~` without `-`, `\` and `'` | 91 | ≈6.5 | 1 | 19% | 1 | 528 / 775 |
| `base94` | `!` to `~` | 94 | 6.55 | 1 | 18% | 1 | 49 / 177 ¹ |
| `braille` | U+2800–U+28FF | 256 | 8 | 3 | 67% | 1 | 406 / 192 |
| `two-byte` | letters of U+0080–U+07FF (Latin, IPA, Greek, Cyrillic, Armenian) ² | 1024 | 10 | 2 | 38% | 1 ³ | 305 / 98 |
| `one-cell` | one-cell characters of U+0800–U+FFFF (scripts from Devanagari on, symbols, arrows, math) ² | 8192 | 13 | 3 | 46% | 1 | 377 / 91 |
| `cjk` | U+3400–U+4DBF, U+4E00–U+9FFF, U+AC00–U+C03F | 32768 | 15 | 3 | 38% | 2 | 245 / 154 |
| `cjk-ext-b` | the above, U+C040–U+D7A3, U+20000–U+2689B | 65536 | 16 | 3 or 4 | 41% | 2 | 237 / 138 |

¹ Reads each piece as one big number, so the work grows with the square of the piece's length.
orimoji folds a whole-file body in pieces of a line's width, about 75 bytes, and these were timed
that way. Every other alphabet takes a few bits at a time.

² The first 1,024 and 8,192 that keep to [the rules below](#what-a-character-must-be), in code
point order, generated by `tools/alphabet_tables.py` and frozen in `src/alphabet/tables.rs`: text
folded with them must unfold forever.

³ Greek, Cyrillic and some Latin-1 letters are "ambiguous width": a terminal set up for East Asian
text draws them two cells wide. No 1024-letter two-byte alphabet avoids them all.

*Bytes wasted* is the share of the stored bits a character leaves unused: a base64 character holds
6 bits in a byte's 8, and a braille one 8 bits in three bytes' 24.

The alphabets past 8 bits a character have a few more characters than their count, a *short set*:
folding bytes, a last character that would leave a whole byte of padding is written in a short one
instead, which holds 8 bits fewer, so the bytes come back exactly. `cjk` and `cjk-ext-b` are the
idea behind qntm's base32768 and base65536.

## Plain text, and the space it wastes

Text as people write it spends a byte or more on each character, but uses few of the values those
bytes can hold:

| Text of | Characters | Count | Bits per character | UTF-8 bytes | Bytes wasted | Packed into base64 |
|---|---|--:|--:|--:|--:|--:|
| bits | `0 1` | 2 | 1 | 1 | 88% | 17% |
| decimal digits | `0-9` | 10 | 3.32 | 1 | 58% | 55% |
| hex digits | `0-9 a-f` | 16 | 4 | 1 | 50% | 67% |
| lowercase letters | `a-z` | 26 | 4.70 | 1 | 41% | 78% |
| lowercase letters and digits | `a-z 0-9` | 36 | 5.17 | 1 | 35% | 86% |
| letters and digits | `a-z A-Z 0-9` | 62 | 5.95 | 1 | 26% | 99% |
| printable ASCII | space to `~` | 95 | 6.57 | 1 | 18% | 109% |
| Hebrew letters | `א` to `ת` | 27 | 4.75 | 2 | 70% | 40% |
| Russian letters | `А` to `я`, `Ё` and `ё` | 66 | 6.04 | 2 | 62% | 50% |
| braille art | U+2800–U+28FF | 256 | 8 | 3 | 67% | 44% |
| box-drawing art | U+2500–U+257F | 128 | 7 | 3 | 71% | 39% |
| block art | U+2580–U+259F | 32 | 5 | 3 | 79% | 28% |
| everyday Chinese | the 3500 of China's list of commonly used characters | 3500 | 11.77 | 3 | 51% | 65% |

*Packed into base64* codes each character in just the bits its set needs, as the `pack` squeeze
does (a line of digits becomes one number in base 10), folds that into base64, and compares it with
the text as UTF-8. It works line by line, so the text still diffs.

These are the least a text wastes. Real text wastes more, since it uses its characters unevenly
and repeats itself, and that is what the other squeezes take back. Packing alone does nothing for
text that already uses most of ASCII, and harms text that uses more than 256 characters, as the
Steam listing shows.

## Words from a fixed list

Packing still pays for every spelling its characters allow, and most never occur: no weekday is
spelled `xqzv`. When a field holds one word from a fixed list, list the words that do occur, and
write each as its place in that list. The number costs only the bits the list's length needs,
however long the word is:

| A word from | Words | Average length | As text, bits | Packed, bits | As an index, bits | Saved over text |
|---|--:|--:|--:|--:|--:|--:|
| `true`, `false` | 2 | 4.5 | 36 | 21.2 | 1 | 97% |
| log levels, `error` to `trace` | 5 | 4.6 | 36.8 | 21.6 | 2.32 | 94% |
| weekdays | 7 | 7.14 | 57.1 | 33.6 | 2.81 | 95% |
| HTTP methods | 9 | 4.89 | 39.1 | 23.0 | 3.17 | 92% |
| months | 12 | 6.17 | 49.3 | 29.0 | 3.58 | 93% |
| cracklib's small word list | 54,763 | 8.0 | 64.0 | 43.4 | 15.74 | 75% |

*Packed* is each character in just the bits its set needs: 4.70 for a letter, or 5.43 for the 43
characters the word list uses.

`orimoji::words` does this for lists a program gives it (`cargo run --example fixed_words`), and
the table squeeze does it by itself, learning each column's list from the file:

- **The list must keep its meaning.** Words may be added at its end, but never removed or
  reordered, or old text unfolds into other words. The table squeeze writes its lists into the
  folded file, and `--like` keeps them as they were.
- **Several fields make one number.** A weekday and a month are one of 7 × 12 = 84 pairs, so one
  number below 84 holds both, in 6.39 bits rather than 3 + 4 whole ones; a `words::Record` of any
  fields works the same way.
- **A word off the list needs a way out:** the table squeeze keeps `<esc>` in every list, followed
  by the value spelled out.
- **The same trick runs backwards:** BIP-39 turns every 11 bits into one of 2048 words, so that
  people can write a key down.

## Against base64, for the same data

| Alphabet | Characters | Bytes | Width | In git: squeezed data | In git: crumpled art |
|---|--:|--:|--:|--:|--:|
| `hex` | 149% | 149% | 150% | 103% | 101–103% |
| `base32` | 120% | 120% | 120% | 100% | 102–108% |
| `base58` | 103% | 103% | 103% | 101% | 107–118% |
| **`base64url`** | **100%** | **100%** | **100%** | **100%** | **100%** |
| `z85` | 94% | 94% | 94% | 101% | 109–117% |
| `base91` | 93% | 93% | 92% | 101% | 108–116% |
| `base94` | 92% | 92% | 92% | 101% | 107–118% |
| `braille` | 75% | 223% | 75% | 142% | 138–146% |
| `two-byte` | 62% | 122% | 61% | 122% | 123–132% |
| `one-cell` | 48% | 140% | 47% | 129% | 129–140% |
| `cjk` | 42% | 122% | 81% | 121% | 121–131% |
| `cjk-ext-b` | 38% | 128% | 75% | 126% | 127–135% |

*Squeezed data* is 64 lines of 48 random bytes, which is what well-squeezed data looks like.
*Crumpled art* is arterminal's three examples, crumpled, their bits refolded. *In git* is the size
after zlib, which git stores every object with.

Zlib takes back most of what an ASCII alphabet wastes, but not what a multi-byte one does: it codes
byte by byte, so the bytes every character repeats still cost bits. Crumpled art is written in
base64's 6-bit units, so in base64 its repeats stay repeats in the text, where zlib finds them; the
other alphabets cut across those units. So base64 is the smallest in git. Every multi-byte
alphabet costs a fifth more there or worse, and the denser ASCII ones break even on squeezed data
but cost 7–18% more on crumpled art.

## What a character must be

Every alphabet past ASCII keeps to characters that survive being text:

- **No combining marks**, which stack onto the character before instead of taking a cell.
- **No right-to-left letters**, which make a terminal draw the line out of order.
- **No controls, spaces or invisible characters**, nothing unassigned, and no emoji, which some
  terminals draw two cells wide.
- **Nothing that Unicode normalization (NFC) changes**, like the Greek question mark, which turns
  into a semicolon.
- `one-cell` also takes nothing a terminal might draw two cells wide, East Asian "ambiguous"
  characters included.

## Zipped lines

A file squeezed whole changes from the first edit on, so a diff of it says nothing. Squeezed a line
at a time, it diffs like the original, at a price:

| Each line squeezed | Long art | Kirino | Miku | An edit changes |
|---|--:|--:|--:|---|
| deflate, alone (`--lean 0`) | 48% | 87% | 46% | its line |
| deflate, leaning on the line above (`--lean 1`) | 38% | 69% | 21% | its line and the next |
| deflate, leaning on the 4 lines above | 36% | 63% | 20% | its line and the 4 after |
| deflate, leaning on the 16 lines above | 35% | 60% | 19% | its line and the 16 after |
| `text` | 30% | 63% | 20% | its line ¹ |
| arterminal's crumple | 29% | 43% | 18% | its line ¹ |
| *deflate over the whole file* | 24% | 47% | 15% | everything after it |

¹ And the code table the file shares, when the edit brings a character the table lacks, unless
the fold keeps its tables (`--like`).

The deflate rows count the body alone, without the folded file's header line. A code shared by the
whole file beats deflate leaning on its neighbours: deflate pays for its own framing on every line
and finds only what the lines above happen to repeat.

## Choosing

- **Kept in a repository, and diffed line by line:** `table` for columns, `text` for drawings and
  prose, into `base64url`, refolded with `--like` so unchanged lines stay unchanged. Big, regular
  files then get smaller in git too; small drawings only get a smaller checkout.
- **Smallest file, no diffs needed:** `zstd`, or `deflate --whole` in a build without zstd.
- **Narrowest on screen:** `two-byte`, at 61% of base64's width for 122% of its bytes. `one-cell`
  reaches 47% for 140%, but fonts cover it patchily.
- **Fewest characters, where a limit counts them:** `cjk`, at 42% of base64's count, or `cjk-ext-b`
  at 38%, which many systems have no font for. Each character is two cells wide.
- **Typed or read out by people:** `crockford`, which reads back what people type (`o` for `0`,
  `l` for `1`, either case), or `hex`.
- **No lookalikes, and a double-click selects all of it:** `base58`.
- **Unquoted in URLs, file names, JSON and shell words:** `base64url`, `base32` or `hex`.
- **Text from a small set, like digits or hex:** `pack`.
- **Showing its bits:** `braille` draws each byte as its dots, at 2.2 times base64's bytes.

`orimoji measure` folds a file every way and prints the comparison, for files unlike these.

## In code

A library first, like its siblings: the binary is a thin shell over the crate. Without its default
features (`cli`, `zstd`) the crate builds without clap and without zstd's C library.

```rust
use orimoji::{Alphabet, Fold, Squeeze, TableOptions};

let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url);
let folded = orimoji::fold(&std::fs::read("listing.tsv")?, &how)?;
let newer = std::fs::read("listing-next.tsv")?;
let next = orimoji::fold_keeping(&newer, &folded, &how)?; // unchanged rows, unchanged lines
assert_eq!(orimoji::unfold(&next)?, newer);
```

`orimoji::pick` chooses the line squeeze the way `auto` does, `orimoji::measure` measures, and
`Alphabet` folds bytes or bits by itself.

## Examples

```sh
cargo run --example alphabets                  # one sentence in every alphabet
cargo run --example drawing                    # an arterminal drawing, and a repainted row
cargo run --release --example steam_listing    # catalogames' listing: the models, and a new sweep
cargo run --example fixed_words                # weekdays, months and records as numbers
```

The data they read is in `examples/data`: arterminal's three example drawings, and the first 400
rows of catalogames' listing with its header.

## Not done yet

- **Tables for tiny files.** Kirino's code table outweighs its body. Code points written as small
  steps from the one before, as crumple writes its table, would bring it near crumple's 43%.
- **Context.** Every symbol is coded on its own. Coding a tag by the tag before it, or a character
  by the one before, would shrink the listing's tag lists and names further.
- **The row above**, for drawings: in crumple, and as a text squeeze that leans on it.
- **brotli and xz.** Over the whole file they came within a few percent of zstd 19 on the drawings,
  at 1 to 6 MiB/s; zstd is here instead.
- **One alphabet of one- and two-byte characters.** In theory it reaches 81% of base64's
  characters at 90% of its bytes, but it needs arithmetic coding, and in git its gain would likely
  vanish as base94's does.

## Measured how

On one core of a Ryzen 9 9950X3D. The alphabets folded and unfolded 1 MiB of random bytes, best of
5. The listing is catalogames' local `steam-games.tsv` (its sweep of 2026-09-26). The drawings are
arterminal's examples as arterminal saves them. The size in git is zlib at level 6, git's default
for packs. `orimoji measure` makes the result tables; the deflate rows under *Zipped lines* came
from a scratch benchmark of the same deflate, and crumple's from arterminal itself.

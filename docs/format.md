# The folded file

Formats 1 to 3, as `orimoji` writes and reads them. A file is written in the oldest format that
holds it:

- **1:** a folded file that needs nothing outside itself, or an index kept beside it, as every
  indexed fold once was;
- **2:** a folded file that keeps its index inside it, in its tables (see [The index](#the-index));
- **3:** the `.ori` and `.orinv` files the text squeeze writes, which keep a fold's unit with its
  lines (see [.ori and .orinv](#ori-and-orinv)); a fold that keeps its index beside it, as the
  first format did, writes the text squeeze's as a folded file of format 1.

A file of a newer format than the reader's is refused by its number. Why it is built this way is
in [theory.md](theory.md).

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

## Lines

A folded file is UTF-8 text in three parts:

1. **The header line:** `=== orimoji <version> <squeeze> <alphabet> [<option> …] ===`.
2. **The tables:** every line up to `=== body ===`, whatever the squeeze shares between lines.
3. **The body:** every line after `=== body ===`, whatever it looks like. A body line of `base94`
   may well start with `===` itself, so nothing after the body line is read as a marker.

The file ends in a newline, which is not part of the body. A file whose last newline was dropped
reads the same, and so does one whose lines end in `\r\n`.

### Options

| Option | Squeezes | Means |
|---|---|---|
| `whole` | `none`, `deflate`, `zstd` | the body is one stream, folded in pieces |
| `size=<bytes>` | the same | the size of what was folded; an unfold that comes out any other size is refused, and decompression stops past it |
| `lean=<lines>` | `deflate` | a line at a time, each with the `<lines>` lines above as its dictionary |
| `index=<count>.<fingerprint>` | `text`, `table` | the text was folded with an index: inside its tables in format 2, beside it in format 1; see [The index](#the-index) |

### Tables

Table lines are text, grouped under section lines `=== <name> ===`. A squeeze's own sections are
below. Unless the fold was asked to keep them readable, the whole of that text is deflated (raw
deflate, no zlib header) and folded like a whole-file body (see below) under one line,
`=== deflated tables ===`, whenever that is smaller.

Text written into a table line, or kept in the body as it was, is escaped so that nothing in it can
break a line or vanish:

| Written | Stands for |
|---|---|
| `\\` | `\` |
| `\t`, `\n`, `\r`, `\e` | tab, newline, carriage return, ESC |
| `\u{…}` | the character with that hex code point: any other control, the invisible characters (zero widths, bidi controls, the byte order mark, …), and a space at the very end |
| `\<` | a `<` at the very start, since an entry spelled `<end>` is a marker |
| `\@` | an `@` at the very start, since an entry spelled `@3` is a place in the index |
| `\0` | nothing: an empty text, so that an entry is never blank |

In format 2, the tables end in an `=== index ===` section: the index the file was folded with, a
value a line, escaped as above, and a value starting with `=` starting with `\u{3d}` instead, so
that no value reads as a section line. It is deflated with the rest of the tables.

### Body

- **A line at a time** (the `pack`, `text` and `table` squeezes, and `none` and `deflate` without
  `whole`): a body line for every line of the original, the original split on `\n` exactly. An
  empty line folds to an empty body line, and nothing else does.
- **A kept line**, under the table squeeze: a space, then the original line escaped. No alphabet has
  a space, so no folded line starts with one.
- **One stream** (`whole`): the squeezed bytes cut into pieces, each folded on its own into a line
  `--wrap` screen cells wide (100 by default): as many bytes as the alphabet holds in that many
  cells, rounded down.

## Bits

Bits are written most significant first. A body line's bits are folded with the last character
filled out with zeros, and a squeeze knows where its own bits end, so the zeros are never read as
data. The text and table squeezes refuse anything but zeros after a line's last code.

### Codes

- **Huffman codes** are canonical: a table gives each symbol's code length (at most 30) in its
  order, and the codes follow, shortest first, those of one length in the table's order, counting
  up from zero.
- **Elias gamma** writes a number of 1 or more as its length in bits, less one, in zeros, then the
  number itself: 1 is `1`, 2 is `010`, 5 is `00101`.

### The text code

The `text` squeeze's `=== symbols ===` section, and the table squeeze's `=== spell ===` section, are
lines of `<code length>\t<symbol>`. A symbol is `<end>`, `<esc>`, `<run>`, or a text: one character,
or, in an indexed fold, a whole word or a terminal escape (a CSI sequence: `ESC [`, parameter bytes
0x30–0x3F, intermediate bytes 0x20–0x2F, a final byte 0x40–0x7E). A text that is in the index is
written as its place in it, `@<place>`; any other, escaped. Spelling a text writes, in order:

- each character, word or escape that has a symbol, as its code (a word only where it is whole,
  never part of a longer one);
- a character that has none as `<esc>`, then gamma(its code point + 1);
- three or more of the same in a row as the first, then `<run>` and gamma(repeats − 1), at most
  4096 repeats to a `<run>`;
- and finally `<end>`.

### pack

The `=== characters ===` section lists the file's characters, one escaped per line, in code point
order; N is their count, or 2 if there is only one. A line's characters are digits in base N, the
first character the least significant. The line is written as:

1. gamma(the number of full segments + 1), a full segment being 256 characters;
2. each full segment as its number, in exactly as many bits as N²⁵⁶ − 1 takes, least significant
   bit first;
3. the rest, n characters long (possibly none), as the number Nⁿ + c₀ + c₁N + … + cₙ₋₁Nⁿ⁻¹, least
   significant bit first, up to the end of the line. The leading 1 marks the length, and the zeros
   filling out the last character are zeros above the number, changing nothing.

### table

The `=== table ===` section:

```text
separator	\t
keep	#
column	1	delta
column	2	values	spell
column	15	list	,	values	spell
```

- `separator`, escaped, splits a row into cells.
- `keep`, if present, is the prefix of lines kept as they were.
- `column`, one per column, in order: its number, then `list` and the item separator for a column
  of lists, then `values` if it has a values section, then its fallback: `spell`, `number` or
  `delta`.

A `=== column <n> values ===` section lists `<code length>\t<entry>` lines: `<esc>`, then `<end>`
for a list column, then the column's values in their order, each as its place in the index,
`@<place>`. A `=== column <n> numbers ===`
section is one line of 66 code lengths, separated by spaces: bit lengths 0 to 64, then the code for
"not a number".

A row is its cells' codes in column order. A cell, or each item of a list cell, followed by `<end>`
for the list, is coded as:

- its entry's code, if the column has values and the value is one of them;
- otherwise `<esc>` (when the column has values), then the fallback:
  - `spell`: the text code of the `=== spell ===` section;
  - `number`: a canonical decimal number (digits, no leading zero, below 2⁶⁴) as the code of its bit
    length b, then its b − 1 bits below the top one; anything else as the "not a number" code, then
    spelled;
  - `delta`: the same for the distance from the last number seen, zigzagged so that small distances
    either way are small numbers (0, −1, 1, −2, … are 0, 1, 2, 3, …). The last number seen is the
    previous row's in the same column, or the previous item's in the same cell, starting from 0.

### deflate and zstd

`deflate` writes raw deflate. A line at a time, each line's dictionary is the `lean` lines above it,
each followed by `\n`. `zstd` writes one standard zstd frame of the whole file.

## The index

An indexed fold refers to the values of its index by place, `@0` for the first. A folded text that
refers to an index has `index=<count>.<fingerprint>` in its header: the number of values the index
had when the text was folded, and the 32-bit FNV-1a hash, in hex, of those values, each followed by
`\n`. An index fits the text if it has at least that many values and the first `<count>` of them
hash the same, so an index that grew at its end still fits.

In format 2, the index is inside the file, in its tables' `=== index ===` section (see
[Tables](#tables)), and the file unfolds alone. In format 1, it was handed back beside the folded
text, for the caller to keep and give back when unfolding; kept as text, it is a header line and
then a value a line, escaped as above:

```text
=== orimoji index 1 ===
Bethesda
Gearbox Software
Valve
```

Unfolding a format 1 text that needs an index without one, or with one that doesn't fit, is
refused; so is a format 2 text whose index doesn't fit its header.

## Parts

A fold into parts (`orimoji::parts`) has no header and no file: three things, each a line of the
alphabet its caller chose, and the index when one paid. The caller keeps them, and knows the
alphabet; the squeeze is always the text squeeze.

### Choices

Every way the fold could write something one of several ways, and the way it took, in bits;
`orimoji::parts::decisions` reads them back.

| Bits | Hold |
|---|---|
| 4 | the parts' format, `2`; a newer one is refused by its number |
| 3 | `k`: how many low bits of a run's count are written as they are |
| 1 | whether the count's bucket has a Huffman code of its own (`1`) or is in unary (`0`) |
| 4 | how many low bits of a character's step, in the table, are written as they are |
| 2 | how the table writes its code lengths: `0` five bits each, `1` in a Huffman code of their own; another number is refused, as a way this orimoji doesn't know |
| 1 | whether the lines need an index |
| Elias gamma | when they do, how many of the index's values they were folded with |
| 32 | the fingerprint: FNV-1a over `<format> <k> <coded> <step bits> <lengths> <values>.<index fingerprint>\n`, the index's fingerprint as for a folded file's header, in 8 hex digits (`0` for none), then the table as written |

Format 1 had neither the step's bits nor the way of writing lengths, and its fingerprint leaves
them out: its tables wrote steps in Elias gamma and lengths in five bits, as format 2's do with
`0` and `0`. An index that grew at its end still fits, as for a folded file. Choices whose
fingerprint isn't the table's and the index's are refused: one of the parts is damaged, or comes
from another fold.

### The code table

Numbers in Elias gamma, code lengths (from 1 to 30) as the choices say:

- how many characters, plus one; then how many longer texts (indexed words and escapes), plus one;
- for lengths in a code of their own, that code: the longest length, in five bits, then for each
  length up to it the length of its own code, in five bits, 0 for a length no symbol has;
- the code lengths of `<end>`, `<esc>` and `<run>`;
- each character, in code point order: its step from the one before (the first, its code point
  plus one), and its code length. The step less one is in Exp-Golomb: all but its low bits, plus
  one, in Elias gamma, then the low bits as they are, as many as the choices say;
- each longer text, in the order of its place in the index: its step from the place before (the
  first, its place plus one), in Elias gamma, and its code length;
- when counts are coded, how many buckets there are, and each one's code length, in five bits.

Symbols are numbered in that order, and the codes follow from their lengths, as everywhere.

### Lines

A line is spelled as the text squeeze spells it (see [The text code](#the-text-code)), but for two
things. A run's count `c`, the repeats past the first two, is split into its `k` low bits and the
rest: the rest plus one, `h`, is written as its bucket (its length in bits, less one), in unary
(that many zeros, then a one) or in the buckets' code; then `h`'s bits after its leading one; then
the `k` low bits. In unary with `k = 0`, that is a folded file's gamma(repeats − 1). And a writer
may write a unit again instead of a run, wherever that is shorter: a reader takes either.

A line, the table and the choices must each be exactly what folding their bits gives: nothing after
the last code but the zeros that fill out the last character. An empty line stands for an empty
line, and no other line is empty.

## .ori and .orinv

The text squeeze folds into parts, and keeps each fold's unit (its choices, its code table and its
index) with its lines, in format 3. A file folded
alone is one `.ori`, its unit inside it:

```text
=== orimoji 3 text base64url index=2 ===
=== unit ===
IgHKTHHM
CsqhghG1A-RVVPO00t…
God
and
=== body ===
_8wTxNYlmw
…
```

1. **The header line:** the format, `text`, the alphabet, and `index=<values>` when the unit has an
   index of that many values.
2. **The unit:** `=== unit ===`, then the choices, the code table, and each of the index's values,
   escaped as in [Tables](#tables), a line each, as many as the header counts.
3. **The body:** `=== body ===`, then the parts' lines, a line for each line of the text, as in
   [Lines](#lines-1).

Files folded together share one unit, learnt over all of them, kept once in an `.orinv`: the
header line and the unit, and no body. Each file's `.ori` holds only its lines, and names the
`.orinv` and the unit it holds:

```text
=== orimoji 3 text base64url orinv=chapters.orinv unit=3f2a91c0 ===
=== body ===
_8wTxNYlmw
…
```

Of files folded together, those that take less alone are written as if each had been folded
alone, an `.ori` holding its unit; only the files that share name the `.orinv`.

`orinv=` is the `.orinv`'s path from the `.ori`'s own folder, escaped as in [Tables](#tables), and a
space as `\u{20}`, so that the path is one word. `unit=` is the unit's fingerprint, in 8 hex
digits: FNV-1a, 32 bits, of the unit's text (its choices, its table and its escaped values, each
followed by `\n`). An `.ori` is unfolded only with a unit of that fingerprint; with no unit, or
another, it is refused. A writer replaces an `.orinv` only when no file it can find (in the
`.orinv`'s folder and in the folders of the files it is writing) names it with the fingerprint of
the unit it holds.

## Alphabets

The characters of each alphabet are in [theory.md](theory.md#alphabets-to-fold-into). Folding:

- **Bits**, as body lines are: a fixed number of bits a character, the last filled out with zeros.
  `z85`, `base91`, `base58` and `base94` fold the bits' bytes instead.
- **Bytes**, as `none`, `deflate`, `zstd` and the deflated tables are, exactly:
  - up to 8 bits a character, the character count says where the bytes end;
  - past 8 bits, a last group that would leave a whole byte of padding is written in one of the
    alphabet's short set, holding 8 bits fewer;
  - `z85` writes four bytes in five characters, and a short last group of n bytes in its first
    n + 1 characters, as Ascii85 does;
  - `base91` is basE91's own loop;
  - `base58` and `base94` write leading zero bytes as zero digits, then the rest as one number.

The two-byte and one-cell alphabets' characters are frozen in `src/alphabet/tables.rs`; a later
Unicode that would pick differently doesn't change them.

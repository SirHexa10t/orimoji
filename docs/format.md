# The folded file

Format version 1, as `orimoji` writes and reads it. Why it is built this way is in
[theory.md](theory.md).

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
| `index=<count>.<fingerprint>` | `text`, `table` | the text needs an index: see [The index](#the-index) |

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

An indexed fold hands its index back beside the folded text, as a list of values; the folded text
refers to them by place, `@0` for the first. Kept as text, the index is a header line and then a
value a line, escaped as above:

```text
=== orimoji index 1 ===
Bethesda
Gearbox Software
Valve
```

A folded text that refers to an index has `index=<count>.<fingerprint>` in its header: the number
of values the index had when the text was folded, and the 32-bit FNV-1a hash, in hex, of those
values, each followed by `\n`. An index fits the text if it has at least that many values and the
first `<count>` of them hash the same, so an index that grew at its end still fits. Unfolding such
a text without an index, or with one that doesn't fit, is refused.

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

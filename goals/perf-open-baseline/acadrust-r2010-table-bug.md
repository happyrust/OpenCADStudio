# acadrust bug report: R2010+ ACAD_TABLE content parser derails (slow + silent cell loss)

Draft for upstream `OpenAEC-Foundation/acadifc`. Found while profiling Open CAD Studio's
file-open path; full measurement context in [`benchmarks.md`](./benchmarks.md). The root cause is
identified and a fix is implemented and validated against the full sample matrix — see
[Root cause](#root-cause-the-missing-isempty-gate-in-read_cad_value),
[Two fixes worth separating](#two-fixes-worth-separating), and [Validation](#validation).

## Summary

Reading a DWG saved as R2010 or newer, the R2010+ `ACAD_TABLE` content parser loses bit-stream
alignment part-way through the cell list. Two consequences, both silent:

1. **Wrong data**: a 7x3 table yields 9 cells instead of 21. The parser reads `nrows=7` correctly
   but rows 4-7 come back empty.
2. **~280ms burned per affected table**: the misaligned read produces a garbage array count that
   `safe_count` clamps to `MAX_ARRAY_COUNT = 100_000`, and the parser then spins through 100k
   bogus iterations before continuing. Records are located by offset, so nothing errors out.

For a host application this shows up as a drawing that opens 100x slower than an identical
R2007 file, and as table cells that disappear on the next save.

A caveat worth stating up front: the pre-R2010 branch returns all 21 cells for this drawing, but a
field-by-field differential (last section) shows its cell *payloads* are garbage. Neither branch
should be treated as ground truth for table cell content.

## Environment

| item | value |
|---|---|
| crate | `acadrust` at `github.com/OpenAEC-Foundation/acadifc`, first seen on rev `bee1a58`, still reproduces on `8cc4793` (2026-07-26) |
| rustc | 1.99.0-nightly (0e29c21d9 2026-07-21), `--release` |
| OS / CPU | Windows 11 26200, AMD Ryzen 9 7950X |

## Reproduction

Both sample files come from the ACadSharp repository (`samples/`): they are the same drawing saved
in different formats.

```rust
// Cargo.toml: acadrust = { git = "...acadifc", rev = "bee1a58" }
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let started = std::time::Instant::now();
    let doc = acadrust::io::dwg::DwgReader::from_file(&path).unwrap().read().unwrap();
    println!("{path} -> {:.1}ms", started.elapsed().as_secs_f64() * 1000.0);
    for e in doc.entities() {
        if let acadrust::EntityType::Table(t) = e {
            let cells: usize = t.rows.iter().map(|r| r.cells.len()).sum();
            println!(
                "  TABLE {:#X} rows={} columns={} cells={}",
                t.common.handle, t.rows.len(), t.columns.len(), cells
            );
        }
    }
}
```

Output for the R2007 sample:

```
sample_AC1021.dwg -> 11.0ms
  TABLE 0x528 rows=7 columns=3 cells=21
  TABLE 0xA35 rows=4 columns=5 cells=20
```

Output for the R2018 sample (slow, and 7 rows x 3 columns should be 21 cells):

```
sample_AC1032.dwg -> 293.1ms
  TABLE 0x528 rows=7 columns=3 cells=9
  TABLE 0xA35 rows=4 columns=5 cells=20
```

Re-run on rev `8cc4793` (2026-07-26) — unchanged:

```
sample_AC1021.dwg ->  15.6ms   TABLE 0x528 rows=7 columns=3 cells=21
sample_AC1032.dwg -> 263.4ms   TABLE 0x528 rows=7 columns=3 cells=9
```

`PERF=1` attributes essentially the whole parse to that one record
(`type_code = -16` is `OBJ_TABLE`, `object_reader/common.rs:124`):

```
[perf] dwg-build pass2=284.1ms decode=283.6ms records=731 threads=32
[slowrec] handle=0x528 type_code=-16  283.3ms
[slowrec] handle=0xD65 type_code=38    11.2ms
```

731 records in the drawing; one table record accounts for 283ms of the 284ms.

## Where it derails

Tracing the counts inside `read_table_content` (`object_reader/entities.rs:2628`) and the cell
header inside `read_table_cell` (`entities.rs:2591`) for table `0x528`:

```
[tbl] ncols=3 nrows=7                      (header is read correctly)
[tbl]   ncells=3
[cell] state=0x0 custom_data=0 ndata=1     (row 1: three good cells)
[cell] state=0x0 custom_data=0 ndata=1
[cell] state=0x0 custom_data=0 ndata=1
[tbl]   ncells=3
[cell] state=0x0 custom_data=0 ndata=1     (row 2: three good cells)
[cell] state=0x0 custom_data=0 ndata=1
[cell] state=0x0 custom_data=0 ndata=1
[tbl]   ncells=3
[cell] state=0x0 custom_data=0 ndata=1     (row 3, cell 1: still fine)
[cell] state=CONTENT_LOCKED,LINKED,FORMAT_LOCKED,FORMAT_MODIFIED_AFTER_UPDATE
       custom_data=63 ndata=100000         (row 3, cell 2: garbage)
[cell]   took 283.6ms                      (100k iterations of read_custom_table_data)
[cell] state=0x0 custom_data=0 ndata=0
[tbl]   ncells=0                           (rows 4-7 read as empty)
[tbl]   ncells=0
[tbl]   ncells=0
[tbl]   ncells=0
```

So:

- The table header (`ncols=3`, `nrows=7`) and the first 7 cells read correctly.
- The 8th cell (row 3, column 2) reads nonsense: implausible state flags, `custom_data=63`, and an
  `ndata` that `safe_count` clamps from garbage down to 100000.
- The misalignment therefore originates while parsing the 7th cell, inside
  `read_table_cell_content` / `read_cell_style` / `read_cad_value` for that cell, and everything
  after it is lost (rows 4-7 come back with zero cells). It is `read_cad_value`; see
  [Root cause](#root-cause-the-missing-isempty-gate-in-read_cad_value) below.
- The other table in the same drawing (`0xA35`, 4x5) keeps its 20 cells, so the derailment is
  triggered by a particular cell variant rather than by tables in general.

## Both branches disagree (so pick ground truth carefully)

Serializing every entity of the same drawing from both files and diffing field by field on handle
(330 entities matched, 34 differ) shows the pre-R2010 branch is not a safe reference for cell
content. Representative rows, `A` = R2007 file, `B` = R2018 file:

```
Table /rows[1]/cells[0]/contents[0]/value/text          A ""          B "Text"
Table /rows[1]/cells[0]/contents[0]/value/value_type    A "Unknown"   B "String"
Table /rows[0]/cells[2]/rotation                        A 1.36e39     B 0
Table /rows[1]/cells[0]/merge_width                     A 137512992   B 1
Table /rows[3]/cells[]                                  A len 3       B len 0
```

The B side carries plausible values (`"Text"`, `String`, `merge_width = 1`) for the cells it does
read, while the A side returns unparsable payloads even though it produces the right cell count.
In other words: the pre-R2010 branch gets the *shape* right and the *content* wrong; the R2010+
branch gets the content right until it derails and then drops the tail. Validating a fix against
ACadSharp or the ODA spec is therefore worth more than validating it against the other branch.

The same differential surfaces disagreements outside tables — MultiLeader attachment points
(14 entities), MText column data (4), one Spline read as control points on one side and fit points
on the other, and Region/Solid3D wire counts (0 vs 175). Those are catalogued separately in
[`acadrust-version-path-diff.md`](./acadrust-version-path-diff.md).

## Root cause: the missing `IsEmpty` gate in `read_cad_value`

Comparing the whole R2010+ table chain against ACadSharp function by function
(`read_table_content` -> `read_table_cell` -> `read_table_cell_content` / `read_cell_style` ->
`read_cad_value`) turns up exactly one divergence. Everything else reads the same fields in the
same order.

ACadSharp's `readCadValue` (`DwgObjectReader.Objects.cs:355`) reads `Flags (BL)` then
`ValueType (BL)`, and reads the typed body only when the value is not flagged empty:

```csharp
if (!R2007Plus || !value.IsEmpty)   // CadValue.IsEmpty => (Flags & 1) != 0
{
    // ... read the typed body according to ValueType ...
}
// Units (BL), Format (TV), FormattedValue (TV) follow unconditionally
```

`read_cad_value` (`object_reader/entities.rs:2424`) drops that gate deliberately, asserting it
never fires:

```rust
// Read the typed value body. The ACadSharp "IsEmpty" gate that can skip
// this does not fire for real tables — reading it unconditionally keeps
// every cell byte-aligned (verified against real drawings).
match type_code { ... }
```

The assertion does not hold for these drawings. When a cell value carries the empty bit the file
contains no body, so acadrust over-reads — for `ValueType` 0/1 that is one spurious `BL`. The
trailing `Units`/`Format`/`FormattedValue` are then read at the wrong offset, and since
`FormattedValue` is a length-prefixed string, a wrong length throws the stream position far off
rather than a few bits.

That reproduces the trace above exactly: the over-read happens while reading cell 7 (row 3,
column 1), so cell 7's own header still looks sane; cell 8's header lands on garbage
(`custom_data=63`, `ndata` clamped to 100000); the 100k-iteration spin burns 283ms; and rows 4-7
read as empty, leaving 9 of 21 cells.

## Two fixes worth separating

**1. Root cause (correctness).** Gate the value body on `IsEmpty` the way ACadSharp does. Routing
the empty case to the `match`'s existing no-op arm keeps the change to the type selector:

```rust
let body_type = if version.r2007_plus() && (v.flags & 1) != 0 {
    -1 // IsEmpty: no value body is present in the stream
} else {
    type_code
};
match body_type { /* arms unchanged; -1 falls through to `_ => {}` */ }
```

On a drawing whose cell values never set the empty bit this is a no-op — both variants read the
body — so it cannot regress whatever drawings the current unconditional read was verified against.

**2. Robustness (cheap, independent).** `safe_count` (`object_reader/mod.rs:33`) currently clamps
to a fixed 100000:

```rust
const MAX_ARRAY_COUNT: i32 = 100_000;
fn safe_count(raw: i32) -> i32 { raw.max(0).min(MAX_ARRAY_COUNT) }
```

Every array element consumes at least one bit, so any count larger than the stream's remaining bit
budget is impossible by construction. Clamping against the remaining stream turns this 283ms
garbage loop into an immediate bail-out and bounds the damage of every other misparse in the reader
the same way. Hanging it off the reader avoids a two-phase borrow at each call site, so the 75
`safe_count(reader.read_bit_long())` sites in `object_reader` become one-line swaps:

```rust
// DwgMergedReader
pub fn read_bounded_count(&mut self) -> i32 {
    let raw = self.read_bit_long();
    let remaining_bits = self.remaining_bytes().saturating_mul(8).min(i32::MAX as usize) as i32;
    raw.max(0).min(100_000).min(remaining_bits)
}
```

This only tightens counts that are impossible by construction, so legitimate counts are unaffected.
It does not fix the wrong cell count on its own — fix 1 does that — but it makes any future
misparse surface as a short read rather than a stall.

## Validation

Both fixes applied on top of rev `8cc4793`; release build; the full ACadSharp sample matrix, which
is the same drawing saved in seven formats. Every file has 341 entities and two tables, `0x528`
(7x3 = 21 cells) and `0xA35` (4x5 = 20 cells). `0xA35` reads 20 cells throughout and is unaffected.

| sample | version | `0x528` cells before | after | parse before | parse after |
|---|---|---|---|---|---|
| sample_AC1014.dwg | R14 | 21 | 21 | 8.5ms | 10.0ms |
| sample_AC1015.dwg | R2000 | 21 | 21 | 5.4ms | 5.3ms |
| sample_AC1018.dwg | R2004 | 21 | 21 | 7.2ms | 6.8ms |
| sample_AC1021.dwg | R2007 | 21 | 21 | 6.6ms | 7.2ms |
| sample_AC1024.dwg | R2010 | 9 | **21** | 271.5ms | **7.0ms** |
| sample_AC1027.dwg | R2013 | 9 | **21** | 262.2ms | **6.0ms** |
| sample_AC1032.dwg | R2018 | 9 | **21** | 261.5ms | **6.4ms** |

Two things worth noting against the earlier sections of this report, which only measured the R2007
and R2018 samples. First, all three R2010+ samples are affected identically, not just R2018 — the
break tracks the R2010+ code path, as the root cause predicts. Second, the pre-R2010 samples are
byte-for-byte unchanged in both cell count and timing, confirming the gate is inert on that path.

No panics, no read errors, and the fork builds without warnings.

## Impact on the host application

- Open time: an R2018 drawing with one affected table takes ~290ms to parse versus ~11ms for the
  same drawing as R2007; four other R2018 samples without such tables parse in 0.4-2.4ms.
- Data loss: opening and re-saving writes the table back with the truncated cell set, so a
  round-trip through any acadrust-based host silently drops table content.

# acadrust correctness sweep: R2007 vs R2018 read paths on the same drawing

Companion to [`acadrust-r2010-table-bug.md`](./acadrust-r2010-table-bug.md). The table bug was
found by profiling; this sweep asks the broader question it raised: **where else do the two DWG
read paths disagree?**

Headline: 34 of 330 entities differ, but every candidate examined so far except one turned out to
be the two files legitimately storing different things. **The only confirmed reader defect is the
ACAD_TABLE one.** Read the status label on each finding before acting on it.

## Method

`sample_AC1021.dwg` (R2007) and `sample_AC1032.dwg` (R2018) from the ACadSharp `samples/` folder
are the same drawing saved in two formats, so any field that differs after parsing means either one
of the two code paths is wrong, or the file itself differs.

The probe serializes every entity with acadrust's `serde` feature, matches entities across the two
documents by handle, walks the two JSON trees field by field, and aggregates the differing paths by
(entity type, JSON path). Arrays of unequal length report their lengths instead of every element.
Floating point is compared with a relative tolerance of 1e-6.

Scope: 330 entities matched by handle, 34 differ. Three handles exist only in each file
(anonymous-block renumbering across saves — expected). Below, `A` is R2007 and `B` is R2018.

**The probe finds divergence, not defects.** Everything it flags needs a second step: dump both
payloads and decide whether they contradict each other or merely differ in shape. Three of the four
candidates chased in this sweep dissolved at that second step.

## Confirmed defect: Table cell content

Covered in the companion report, and the only unambiguous breakage found: the R2010+ path reads
plausible cell content but derails after 7 cells and returns rows 4-7 empty (9 cells instead of 21)
while burning ~280ms in a `safe_count`-clamped garbage loop; the pre-R2010 path keeps all 21 cells
but fills them with unusable payloads (`content_type = "Unknown"`, empty text,
`merge_width = 137512992`, `rotation = 1.36e39`).

## Not a defect: EED that became native fields (23 entities)

`/common/extended_data/records[]` reads `len 1` on A and `len 0` on B. The payloads explain it:

```
EED 0x3EC MText
  A: [{"application_name":"ACAD","values":[{"String":"ACAD_MTEXT_COLUMN_INFO_BEGIN"},...,
      {"Integer16":48},{"Real":15.741414244987482},{"Integer16":49},{"Real":110.22316703811953},...]}]
  B: []

EED 0x7B8 MultiLeader
  A: [{"application_name":"ACAD_MLEADERVER","values":[{"Integer16":2}]}]
  B: []
```

The MText column numbers in A's EED (`15.741…`, `110.223…`) are exactly B's native
`/column_data/width` and `/column_data/gutter`. R2010+ carries the multileader version, the spline
knot parameterization and the MText column block in the record itself; R2007 carried them in EED.
Both readers are right for their own format. This also accounts for the paired findings
`MText /column_data/*` (A zero, B populated) and `Spline /knot_parameterization` (A 0, B 15).

## Not a defect: Spline stored two different ways

This one looked like a solid bug and was not. The reasoning is worth keeping.

The diff showed `0x434` as 4 control points + 8 knots on A versus 2 fit points on B, and
`read_spline` (`object_reader/entities.rs:690`) has a suspicious-looking heuristic that overrides
the record's own storage selector on R2013+:

```rust
// "R2013+ encodes the storage method in splineflags1, not the leading
//  scenario field (which is unreliable here)"
if _flags1 & 1 != 0 { scenario = 2; }        // fit points
else if _flags1 & 2 != 0 { scenario = 1; }   // control vertices
```

Instrumentation showed the record's own selector says control vertices while the override flips it:

```
[spline] raw_scenario=1 -> scenario=1 flags1=0  knot_param=15    (spline 0x433)
[spline] raw_scenario=1 -> scenario=2 flags1=9  knot_param=2     (spline 0x434)
```

Two experiments settled it:

1. **Removing the override made things worse.** The control-vertex branch then read `knots len 222`,
   `control_points len 0`, `knot_tolerance 1` — garbage. The R2018 record really does hold fit data,
   and the override reads the format correctly.
2. **The two representations describe the same curve:**

```
A (R2007): control_points = (250.4588, 17.4260) (254.7138, 28.2550)
                            (259.3880, 17.5031) (259.2246, 27.2875)
           knots = [0,0,0,0,1,1,1,1]   degree 3
B (R2018): fit_points    = (250.4588, 17.4260) (259.2246, 27.2875)
           begin_tangent = (12.7649, 32.4869)   end_tangent = (-0.4902, 29.3531)
```

B's two fit points are exactly A's first and last control points, and a cubic with knot vector
`[0,0,0,0,1,1,1,1]` is a single Bezier segment — precisely a two-point fit spline with end
tangents. Same curve, two encodings.

## Not a defect: Text layer `"0 @ 1"` (the files really differ)

`/common/layer` reads `"0 @ 1"` on A and `"0"` on B for two Text entities. Everything else in those
entities is byte-identical after parsing — insertion point, text value, style, handles — so nothing
was misaligned. Layer names are resolved through the layer table, and dumping it shows why:

```
A (R2007): … 0x9F4:color_125_33_79  0xEFC:0 @ 1  0xF02:Layer_color_80 @ 1
B (R2018): … 0x9F4:color_125_33_79
```

The R2007 file carries two extra `"… @ 1"` layers — the compatibility layers AutoCAD emits to
express per-viewport layer property overrides in formats that lack native support for them. The two
Text entities legitimately point at `0xEFC` in that file and at plain `0` in the R2018 file. Both
readers are correct; the files differ.

## Unverified: status unknown

Each still needs the payload-dump step before anyone calls it a bug. Given the record above, expect
some of these to be file differences too.

- **MultiLeader `/text_attachment_point`**: `Center` on A, `Left` on B, 14 of 15 entities. One
  outlier (`0xB27`) also disagrees on `text_attachment_direction`, both left/right attachments,
  `dogleg_length` (0 vs 8), `landing_distance` (0 vs 8) and `content_base_point`.
  `/dwg_handle_bits` (0 vs 118) is round-trip bookkeeping; ignore it.
- **Region / Solid3D** (3 entities): `/wires[]` len 0 vs 175 and 0 vs 124; `/acis_data/sab_data[]`
  2861 vs 2876 and 7687 vs 7779; `/acis_data/wireframe_isolines` 0 vs -924823351. The negative
  isoline count is the least plausible value in the whole sweep and is the one worth checking first.
- **Insert `0x79C` / AttributeDefinition `0x796`**: `tag` `"MULTI_LINE_ATT_001"` vs
  `"MULTI_LINE_ATT"`, `field_length` 0 vs 293, `preset` false vs true, `vertical_alignment`
  `Baseline` vs `Top`, alignment point (0,0) vs a real coordinate. Only the drawing's one multi-line
  attribute is affected.
- **Benign**: `Insert /block_name` `"*U2"` vs `"*U1"` (3 entities) is anonymous-block renumbering.

## Reproducing

The probe lives outside this repository (`%TEMP%\acadrust_probe`): a copy of the acadrust source
plus a small binary crate that path-depends on it. A full build is about 35 seconds, incremental
about 3, so the differential is cheap to re-run after any reader change and doubles as a regression
test. A third argument that is a handle (`0x434`) dumps that entity's full JSON from both files;
`--layers` dumps both layer tables. Those two dumps are what turn a flagged difference into a
verdict.

## Method note

Four candidates were chased here. One was a real defect (tables); three were not: the EED
migration, the spline storage form, and the `"0 @ 1"` layer. Two of the three looked convincing
before the payload was dumped, and one of them had a plausible-looking root cause in the source
that turned out to be correct code. An earlier draft of the table report made the opposite error —
it treated the R2007 cell count as ground truth, and the field dump showed those cells were garbage.

When two implementations of the same format disagree, the flagged difference is a question, not an
answer, and the default assumption must be that **either side may be wrong, or neither**.

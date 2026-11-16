// What `SvgSink` wrote, read back from the file in sheet millimetres.
//
// Test support for callers outside this module: the automation tests plot
// through the whole application and then ask what reached the page. They
// cannot ask attribute by attribute on a `<path>`, the way upstream's tests
// ask upstream's writer, because this writer does not lay a page out like
// that:
//
//   - paint is on the group a run of paths shares, not on each path;
//   - path data is in the emitter's points, under the page's transform groups
//     (points → mm with the flip, then the plot CTM);
//   - a clip is a path in `<defs>`, in the coordinates of the group that uses
//     it.
//
// These functions undo exactly that and nothing more. The acceptance of the
// writer itself is `tests.rs`, which reads the file with usvg.

use roxmltree::{Document, Node};

/// A presentation attribute as it applies to `node`: its own, or that of the
/// nearest ancestor that sets it.
pub(crate) fn inherited<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.ancestors().find_map(|at| at.attribute(name))
}

/// The paths that draw, in document order. The path of a `<clipPath>` sits in
/// `<defs>` and draws nothing.
pub(crate) fn drawn_paths<'a, 'i>(document: &'a Document<'i>) -> Vec<Node<'a, 'i>> {
    document
        .descendants()
        .filter(|node| {
            node.tag_name().name() == "path"
                && !node.ancestors().any(|at| at.tag_name().name() == "defs")
        })
        .collect()
}

/// The first drawn path that is stroked.
pub(crate) fn first_stroked_path<'a, 'i>(document: &'a Document<'i>) -> Option<Node<'a, 'i>> {
    drawn_paths(document)
        .into_iter()
        .find(|path| inherited(*path, "stroke").is_some_and(|stroke| stroke != "none"))
}

/// Where the points of a drawn path land on the sheet: millimetres, Y down,
/// as the root's `viewBox` has them. Control points of a `C` span are points
/// like any other here.
pub(crate) fn sheet_points(path: Node<'_, '_>) -> Vec<[f64; 2]> {
    pairs(path.attribute("d").expect("path data"))
        .into_iter()
        .map(|point| to_sheet(path, point))
        .collect()
}

/// The box `[x0, y0, x1, y1]`, in sheet millimetres, of every clip the page
/// applies besides the one at its own edge, in document order.
pub(crate) fn clip_boxes(document: &Document<'_>) -> Vec<[f64; 4]> {
    document
        .descendants()
        .filter_map(|user| {
            let id = user
                .attribute("clip-path")?
                .strip_prefix("url(#")?
                .strip_suffix(')')?;
            if id == "page-clip" {
                return None;
            }
            let clip = document
                .descendants()
                .find(|node| {
                    node.tag_name().name() == "clipPath" && node.attribute("id") == Some(id)
                })
                .unwrap_or_else(|| panic!("clip #{id} is used and not defined"));
            // `userSpaceOnUse`: the transforms that place the clip are those
            // of the group using it, not of `<defs>`.
            let mut bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for path in clip
                .children()
                .filter(|node| node.tag_name().name() == "path")
            {
                for point in pairs(path.attribute("d").expect("clip path data")) {
                    let [x, y] = to_sheet(user, point);
                    bounds = [
                        bounds[0].min(x),
                        bounds[1].min(y),
                        bounds[2].max(x),
                        bounds[3].max(y),
                    ];
                }
            }
            Some(bounds)
        })
        .collect()
}

/// A point in the coordinates of `node`, carried through every `transform`
/// from the node outwards.
fn to_sheet(node: Node<'_, '_>, [mut x, mut y]: [f64; 2]) -> [f64; 2] {
    for at in node.ancestors() {
        if let Some(transform) = at.attribute("transform") {
            let [a, b, c, d, e, f] = matrix(transform);
            (x, y) = (a * x + c * y + e, b * x + d * y + f);
        }
    }
    [x, y]
}

/// `matrix(a,b,c,d,e,f)`: the only transform the writer puts on a group.
fn matrix(transform: &str) -> [f64; 6] {
    let coefficients: Vec<f64> = transform
        .strip_prefix("matrix(")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("a transform that is not one matrix: {transform}"))
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("matrix coefficient {value:?} in {transform}"))
        })
        .collect();
    coefficients
        .try_into()
        .unwrap_or_else(|_| panic!("a matrix without six coefficients: {transform}"))
}

/// The coordinate pairs of path data. The writer's numbers are plain
/// decimals, so a letter is always a command and never an exponent.
fn pairs(data: &str) -> Vec<[f64; 2]> {
    let numbers: Vec<f64> = data
        .split(|c: char| c.is_ascii_alphabetic() || c == ',' || c.is_whitespace())
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse()
                .unwrap_or_else(|_| panic!("path coordinate {value:?} in {data}"))
        })
        .collect();
    let (pairs, odd) = numbers.as_chunks::<2>();
    assert!(odd.is_empty(), "an odd number of coordinates in {data}");
    pairs.to_vec()
}

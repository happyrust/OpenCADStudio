//! What a model item is called in the tree.
//!
//! An item's identity is a 32-digit UID, which tells a reader nothing. What
//! a drawing publishes for it usually includes a name -- the tag of an
//! instrument (`MM-222`), of a vessel (`IE-41012C`), the text of a note --
//! and that is what the tree shows. An item that publishes none is shown as
//! its type and its number among the unnamed ones (`PipeRun #17`); the UID
//! stays in the details either way.

use super::{DrawingItem, ItemAttribute};

/// How much of a long name (a note's text) a row shows.
const NAME_LIMIT: usize = 48;

/// The name an item publishes: its `IObject.Name`, or the text of a note.
/// One line, cut to [`NAME_LIMIT`] characters.
pub fn published_name(attributes: &[ItemAttribute]) -> Option<String> {
    let value = |interface: &str, name: &str| {
        attributes
            .iter()
            .find(|attribute| attribute.interface == interface && attribute.name == name)
            .map(|attribute| attribute.value.as_str())
            .filter(|value| !value.trim().is_empty())
    };
    let raw = value("IObject", "Name").or_else(|| value("INote", "NoteText"))?;
    // A note may run over several lines; the row takes them as one.
    let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= NAME_LIMIT {
        return Some(line);
    }
    let mut cut: String = line.chars().take(NAME_LIMIT).collect();
    cut.push('…');
    Some(cut)
}

/// Order the items of one type for the tree and give each its label: the
/// named ones first, by name as a person sorts it, then the unnamed ones by
/// UID, numbered from one.
pub fn label_items(item_type: &str, items: &mut Vec<DrawingItem>) {
    let mut named: Vec<(String, DrawingItem)> = Vec::new();
    let mut unnamed: Vec<DrawingItem> = Vec::new();
    for item in items.drain(..) {
        match published_name(&item.attributes) {
            Some(name) => named.push((name, item)),
            None => unnamed.push(item),
        }
    }
    named.sort_by(|left, right| {
        natural_cmp(&left.0, &right.0).then_with(|| left.1.uid.cmp(&right.1.uid))
    });
    unnamed.sort_by(|left, right| left.uid.cmp(&right.uid));
    for (name, mut item) in named {
        item.label = name;
        items.push(item);
    }
    for (at, mut item) in unnamed.into_iter().enumerate() {
        item.label = format!("{item_type} #{}", at + 1);
        items.push(item);
    }
}

/// Text as a person sorts it: case does not matter, and a run of digits
/// counts as the number it spells, so `P-2` comes before `P-10`.
pub fn natural_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut a = left.chars().peekable();
    let mut b = right.chars().peekable();
    let digits = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| -> String {
        let mut run = String::new();
        while let Some(ch) = chars.peek().copied().filter(char::is_ascii_digit) {
            run.push(ch);
            chars.next();
        }
        run
    };
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return left.cmp(right),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x, y) = (digits(&mut a), digits(&mut b));
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let by_number = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if by_number != Ordering::Equal {
                    return by_number;
                }
            }
            (Some(x), Some(y)) => {
                let by_char = x.to_lowercase().cmp(y.to_lowercase());
                if by_char != Ordering::Equal {
                    return by_char;
                }
                a.next();
                b.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    fn attribute(interface: &str, name: &str, value: &str) -> ItemAttribute {
        ItemAttribute {
            interface: interface.to_string(),
            name: name.to_string(),
            value: value.to_string(),
        }
    }

    fn item(uid: &str, attributes: Vec<ItemAttribute>) -> DrawingItem {
        DrawingItem {
            uid: uid.to_string(),
            label: String::new(),
            item_type: "Instrument".to_string(),
            class: None,
            attributes,
        }
    }

    #[test]
    fn an_item_is_called_by_the_name_it_publishes() {
        assert_eq!(
            published_name(&[
                attribute("INamedInstrument", "MeasuredVariable", "MM"),
                attribute("IObject", "Name", "MM-222"),
            ]),
            Some("MM-222".to_string())
        );
        // A note has no name of its own; its text stands in, on one line.
        assert_eq!(
            published_name(&[attribute("INote", "NoteText", "see  sheet 3\nfor the vent")]),
            Some("see sheet 3 for the vent".to_string())
        );
        // An empty name is no name, and neither is another interface's.
        assert_eq!(published_name(&[attribute("IObject", "Name", "  ")]), None);
        assert_eq!(
            published_name(&[attribute("INamedEquipment", "Name", "V-1")]),
            None
        );
        assert_eq!(published_name(&[]), None);
    }

    #[test]
    fn a_long_note_is_cut_to_a_rows_length() {
        let text = "一二三四五六七八九十".repeat(6);
        let name = published_name(&[attribute("INote", "NoteText", &text)]).unwrap();
        assert_eq!(name.chars().count(), NAME_LIMIT + 1);
        assert!(name.ends_with('…'));
        assert!(text.starts_with(name.trim_end_matches('…')));
    }

    #[test]
    fn named_items_come_first_by_name_and_the_rest_are_numbered_by_uid() {
        let named = |uid: &str, name: &str| item(uid, vec![attribute("IObject", "Name", name)]);
        let mut items = vec![
            item("UID-9", Vec::new()),
            named("UID-5", "P-10"),
            item(
                "UID-2",
                vec![attribute("INamedInstrument", "MeasuredVariable", "T")],
            ),
            named("UID-7", "p-2"),
            named("UID-1", "P-10"),
        ];
        label_items("Instrument", &mut items);
        let shown: Vec<(&str, &str)> = items
            .iter()
            .map(|item| (item.label.as_str(), item.uid.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                // `p-2` before `P-10`: by number, whatever the case. Two
                // items of one name keep UID order.
                ("p-2", "UID-7"),
                ("P-10", "UID-1"),
                ("P-10", "UID-5"),
                ("Instrument #1", "UID-2"),
                ("Instrument #2", "UID-9"),
            ]
        );
    }

    #[test]
    fn names_sort_the_way_a_person_reads_them() {
        assert_eq!(natural_cmp("P-2", "P-10"), Ordering::Less);
        assert_eq!(natural_cmp("LG-0203", "lg-203a"), Ordering::Less);
        assert_eq!(natural_cmp("A", "a"), Ordering::Less);
        assert_eq!(natural_cmp("V-1", "V-1"), Ordering::Equal);
    }
}

//! Objects by id, path and glob (spec: agent toolkit R1, 5.1). An id is `e<n>`, `g<n>` or `m<n>`
//! from the session-unique `ObjId`; a path is the names from the top of the outliner joined with
//! `/`, which a glob can match in part: `*` and `?` stay inside a name, `**` crosses `/`.

use std::collections::HashMap;

use cuboid_core::{Core, Target};
use cuboid_model::{Document, ObjRef};
use serde::Deserialize;

use crate::error::ToolError;

/// One selector or a list of them, as a tool takes it.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Selector {
    /// An id, a path or a glob.
    One(String),
    /// Several; each of them has to match something.
    Many(Vec<String>),
}

impl Selector {
    fn items(&self) -> &[String] {
        match self {
            Selector::One(item) => std::slice::from_ref(item),
            Selector::Many(items) => items,
        }
    }
}

/// The objects of a document with their ids and paths, in outliner order.
pub struct Tree {
    order: Vec<ObjRef>,
    paths: HashMap<ObjRef, String>,
    by_id: HashMap<String, ObjRef>,
}

/// The id of an object: `e12`, `g3` or `m7`. The blueprint and the model origin have none.
pub fn id_of(obj: ObjRef) -> Option<String> {
    match obj {
        ObjRef::Element(id) => Some(format!("e{}", id.get())),
        ObjRef::Group(id) => Some(format!("g{}", id.get())),
        ObjRef::Mesh(id) => Some(format!("m{}", id.get())),
        ObjRef::Blueprint | ObjRef::ModelOrigin => None,
    }
}

impl Tree {
    /// Indexes the elements, meshes and groups of a document.
    pub fn of(doc: &Document) -> Tree {
        let mut tree = Tree {
            order: Vec::new(),
            paths: HashMap::new(),
            by_id: HashMap::new(),
        };
        for (node, parent) in doc.outline() {
            let name = match node {
                ObjRef::Element(id) => doc.elements.get(id).map(|e| e.base.label.to_string()),
                ObjRef::Mesh(id) => doc.meshes.get(id).map(|m| m.base.label.to_string()),
                ObjRef::Group(id) => doc.groups.get(id).map(|g| g.name.to_string()),
                ObjRef::Blueprint | ObjRef::ModelOrigin => None,
            };
            let (Some(name), Some(id)) = (name, id_of(node)) else {
                continue;
            };
            let path = match parent.and_then(|up| tree.paths.get(&ObjRef::Group(up))) {
                Some(above) => format!("{above}/{name}"),
                None => name,
            };
            tree.order.push(node);
            tree.paths.insert(node, path);
            tree.by_id.insert(id, node);
        }
        tree
    }

    /// The objects in outliner order.
    pub fn objects(&self) -> &[ObjRef] {
        &self.order
    }

    /// The path of an object.
    pub fn path_of(&self, obj: ObjRef) -> Option<&str> {
        self.paths.get(&obj).map(String::as_str)
    }

    /// What a selector names, each object once, in the order the items name them. An item that
    /// matches nothing is `E_NO_MATCH`; a path that matches more than one object is
    /// `E_AMBIGUOUS`. An empty list names nothing and is `E_INPUT`.
    pub fn select(&self, selector: &Selector) -> Result<Vec<ObjRef>, ToolError> {
        if selector.items().is_empty() {
            return Err(ToolError::input("the selector is an empty list"));
        }
        let mut found: Vec<ObjRef> = Vec::new();
        for item in selector.items() {
            for obj in self.item(item)? {
                if !found.contains(&obj) {
                    found.push(obj);
                }
            }
        }
        Ok(found)
    }

    /// The single object a selector names.
    pub fn one(&self, selector: &Selector) -> Result<ObjRef, ToolError> {
        match self.select(selector)?.as_slice() {
            [obj] => Ok(*obj),
            many => Err(ToolError::ambiguous(format!(
                "the selector names {} objects, one is needed",
                many.len()
            ))),
        }
    }

    fn item(&self, item: &str) -> Result<Vec<ObjRef>, ToolError> {
        if is_id(item) {
            return self
                .by_id
                .get(item)
                .map(|obj| vec![*obj])
                .ok_or_else(|| ToolError::no_match(format!("no object has the id {item}")));
        }
        let glob = item.contains(['*', '?']);
        let matched: Vec<ObjRef> = self
            .order
            .iter()
            .copied()
            .filter(|obj| {
                self.paths.get(obj).is_some_and(|path| {
                    if glob {
                        matches(item, path)
                    } else {
                        path == item
                    }
                })
            })
            .collect();
        if matched.is_empty() {
            return Err(ToolError::no_match(format!("nothing matches {item}")));
        }
        if !glob && matched.len() > 1 {
            return Err(ToolError::ambiguous(format!(
                "{} objects have the path {item}",
                matched.len()
            )));
        }
        Ok(matched)
    }
}

/// `e12`, `g3`, `m7`: a letter and digits.
fn is_id(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some('e' | 'g' | 'm'))
        && !text[1..].is_empty()
        && text[1..].bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether the glob matches the whole text. `?` is one byte and `*` any run of bytes, neither
/// across a `/`; `**` is any run, `/` included. The time is the length of the glob times the
/// length of the text, however many stars there are.
pub fn matches(glob: &str, text: &str) -> bool {
    let (glob, text) = (glob.as_bytes(), text.as_bytes());
    // `head`: what has been read of the glob matches no byte of the text; `rest[at - 1]`: it
    // matches the first `at` bytes.
    let mut head = true;
    let mut rest = vec![false; text.len()];
    let mut next = 0;
    while let Some(&token) = glob.get(next) {
        let double = token == b'*' && glob.get(next + 1) == Some(&b'*');
        next += if double { 2 } else { 1 };
        // The value the row held at the byte before, in the row of the token before.
        let mut diagonal = head;
        head = token == b'*' && head;
        let mut left = head;
        for (cell, &byte) in rest.iter_mut().zip(text) {
            let above = *cell;
            *cell = match token {
                b'*' => above || (left && (double || byte != b'/')),
                b'?' => diagonal && byte != b'/',
                literal => diagonal && byte == literal,
            };
            diagonal = above;
            left = *cell;
        }
    }
    rest.last().copied().unwrap_or(head)
}

/// The command targets of objects, as the core numbers them now.
pub fn targets(core: &Core, objs: &[ObjRef]) -> Vec<Target> {
    objs.iter().filter_map(|&obj| core.target_of(obj)).collect()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn a_star_stays_inside_a_name_and_two_stars_cross_the_slashes() {
        assert!(matches("head/*", "head/trim"));
        assert!(!matches("head/*", "head/trim/inner"));
        assert!(matches("head/**", "head/trim/inner"));
        assert!(!matches("head/**", "head"));
        assert!(matches("**/trim", "head/x/trim"));
        assert!(matches("*", "head"));
        assert!(!matches("*", "head/trim"));
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "a/c"));
        assert!(matches("trim_*_left", "trim_big_left"));
        assert!(!matches("trim", "trim_left"));
    }

    #[test]
    fn only_stars_match_an_empty_name() {
        assert!(matches("", "") && matches("*", "") && matches("**", ""));
        assert!(!matches("a", "") && !matches("?", ""));
    }

    #[test]
    fn many_stars_do_not_make_a_match_slow() {
        let glob = format!("{}b", "*a".repeat(10));
        let name = "a".repeat(40);
        let start = Instant::now();
        assert!(!matches(&glob, &name));
        assert!(matches(&glob, &format!("{name}b")));
        assert!(!matches(&"?*".repeat(500), &name));
        assert!(matches(&"**".repeat(1000), &format!("{name}/{name}")));
        assert!(start.elapsed() < Duration::from_millis(500));
        // A star that stops at a slash next to one that does not still finds its match.
        assert!(matches("**a*/b", "xa/ya/b"));
        assert!(!matches("**a*/b", "xa/y/ab"));
    }

    #[test]
    fn ids_are_a_letter_and_digits() {
        assert!(is_id("e12") && is_id("g3") && is_id("m7"));
        for text in ["e", "x12", "e1a", "E12", "e-1", "head"] {
            assert!(!is_id(text), "{text}");
        }
    }
}

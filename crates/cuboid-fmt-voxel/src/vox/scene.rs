//! The scene graph chunks `nTRN`, `nGRP` and `nSHP`, and the placement of the models they name.
//!
//! The format notes are public and the chunk semantics are **UNVERIFIED** here (interop 31.8):
//! a transform node places its child with a rotation byte `_r` and a translation `_t`, a group
//! lists children, a shape names models. A model is centred on its transform: the voxel at
//! `(x, y, z)` of a model of size `s` sits at `floor((2p + 1 - s) / 2)` before the rotation is
//! taken in doubled coordinates, so a rotation about the centre keeps every cell on the grid.

use std::collections::HashMap;

use super::Limits;

/// A 3 x 3 matrix of 0 and +-1: a rotation of 90 degree steps, possibly mirrored.
pub(crate) type Mat = [[i64; 3]; 3];

pub(crate) const IDENTITY: Mat = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];

/// The deepest nesting of nodes that the walk follows; a loop in the graph hits it.
const MAX_DEPTH: u32 = 64;

/// A reader over the content of a chunk; every read is checked.
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let part = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(part)
    }

    pub(crate) fn id(&mut self) -> Option<[u8; 4]> {
        self.take(4)?.try_into().ok()
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.take(4)?.try_into().ok().map(u32::from_le_bytes)
    }

    fn i32(&mut self) -> Option<i32> {
        self.take(4)?.try_into().ok().map(i32::from_le_bytes)
    }
}

/// A size field as a length; a value that does not fit makes every following read fail.
pub(crate) fn length(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// A node of the scene graph.
#[derive(Debug, Clone)]
pub(crate) enum Node {
    /// Places its child.
    Transform { child: i32, rot: Mat, t: [i64; 3] },
    /// Lists children.
    Group(Vec<i32>),
    /// Holds a model, by its position in the file.
    Shape(Option<usize>),
}

/// A model placed in the scene.
#[derive(Debug, Clone)]
pub(crate) struct Placed {
    pub model: usize,
    pub rot: Mat,
    pub t: [i64; 3],
}

fn string<'a>(c: &mut Cursor<'a>) -> Option<&'a [u8]> {
    let n = length(c.u32()?);
    c.take(n)
}

/// A DICT: pairs of strings. The count is checked against the bytes left before it is trusted.
fn dict<'a>(c: &mut Cursor<'a>) -> Option<Vec<(&'a [u8], &'a [u8])>> {
    let n = length(c.u32()?);
    if n.checked_mul(8)? > c.remaining() {
        return None;
    }
    let mut pairs = Vec::with_capacity(n);
    for _ in 0..n {
        let key = string(c)?;
        let value = string(c)?;
        pairs.push((key, value));
    }
    Some(pairs)
}

/// The rotation byte: bits 0-1 and 2-3 give the column of the +-1 in the first two rows, the third
/// row takes the column that is left, bits 4-6 are the signs of the rows.
fn rotation(byte: u8) -> Option<Mat> {
    let a = usize::from(byte & 3);
    let b = usize::from((byte >> 2) & 3);
    if a > 2 || b > 2 || a == b {
        return None;
    }
    let c = 3 - a - b;
    let row = |column: usize, bit: u8| -> [i64; 3] {
        let sign = if (byte >> bit) & 1 == 1 { -1 } else { 1 };
        std::array::from_fn(|j| if j == column { sign } else { 0 })
    };
    Some([row(a, 4), row(b, 5), row(c, 6)])
}

fn translation(text: &[u8]) -> Option<[i64; 3]> {
    let mut parts = std::str::from_utf8(text).ok()?.split_ascii_whitespace();
    let mut next = || parts.next()?.parse::<i32>().ok().map(i64::from);
    Some([next()?, next()?, next()?])
}

/// Reads a node chunk into its id and node. `None` for a damaged chunk.
pub(crate) fn parse_node(id: &[u8; 4], body: &[u8]) -> Option<(i32, Node)> {
    let mut c = Cursor::new(body);
    let node_id = c.i32()?;
    dict(&mut c)?;
    let node = match id {
        b"nTRN" => {
            let child = c.i32()?;
            let _reserved = c.i32()?;
            let _layer = c.i32()?;
            let frames = c.i32()?;
            let mut rot = IDENTITY;
            let mut t = [0; 3];
            if frames >= 1 {
                for (key, value) in dict(&mut c)? {
                    match key {
                        b"_r" => {
                            let byte = std::str::from_utf8(value).ok()?.trim().parse().ok()?;
                            rot = rotation(byte).unwrap_or(IDENTITY);
                        }
                        b"_t" => t = translation(value).unwrap_or([0; 3]),
                        _ => {}
                    }
                }
            }
            Node::Transform { child, rot, t }
        }
        b"nGRP" => {
            let count = usize::try_from(c.i32()?).ok()?;
            if count.checked_mul(4)? > c.remaining() {
                return None;
            }
            let children = (0..count).map(|_| c.i32()).collect::<Option<_>>()?;
            Node::Group(children)
        }
        _ => {
            let count = c.i32()?;
            let model = if count >= 1 {
                usize::try_from(c.i32()?).ok()
            } else {
                None
            };
            Node::Shape(model)
        }
    };
    Some((node_id, node))
}

/// `a * b`: the rotation `b` first, then `a`.
pub(crate) fn mul(a: &Mat, b: &Mat) -> Mat {
    a.map(|row| {
        row.iter().zip(b).fold([0; 3], |acc, (&s, brow)| {
            [
                acc[0] + s * brow[0],
                acc[1] + s * brow[1],
                acc[2] + s * brow[2],
            ]
        })
    })
}

pub(crate) fn apply(m: &Mat, v: [i64; 3]) -> [i64; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}

/// The models of the graph below node 0 with their composed placements, in walk order. `None`
/// when the graph cannot be followed: no root, nested too deep (a loop), or too many visits.
pub(crate) fn instances(
    nodes: &HashMap<i32, Node>,
    models: usize,
    limits: &Limits,
) -> Option<Vec<Placed>> {
    nodes.get(&0)?;
    let mut found = Vec::new();
    let mut stack = vec![(0, IDENTITY, [0_i64; 3], 0_u32)];
    let mut visits = 0_u32;
    while let Some((id, rot, t, depth)) = stack.pop() {
        visits += 1;
        if visits > limits.max_visits || depth > MAX_DEPTH {
            return None;
        }
        match nodes.get(&id) {
            Some(Node::Transform {
                child,
                rot: own,
                t: shift,
            }) => {
                let moved = apply(&rot, *shift);
                let at = [t[0] + moved[0], t[1] + moved[1], t[2] + moved[2]];
                stack.push((*child, mul(&rot, own), at, depth + 1));
            }
            Some(Node::Group(children)) => {
                // Pushed in reverse, so the first child is walked first.
                for &child in children.iter().rev() {
                    stack.push((child, rot, t, depth + 1));
                }
            }
            Some(&Node::Shape(Some(model))) if model < models => {
                found.push(Placed { model, rot, t });
            }
            _ => {}
        }
    }
    Some(found)
}

//! The chain of parent models: files in the same folder tree, then the built-in parents
//! (interop 31.2: at most 32 levels, a loop ends the chain, the `generated` family ends it).

use std::collections::HashSet;
use std::path::Path;

use cuboid_io::texture_files::TextureFiles;
use serde_json::Value;

use super::builtin::{self, Builtin};
use super::parse;
use crate::jv::{safe_path, text};
use crate::{Limits, Notice};

/// One model of the chain.
pub(crate) struct Layer {
    pub json: Value,
    /// The folder of its file with `/` separators and a trailing `/`; empty for a built-in model.
    pub folder: String,
    pub builtin: bool,
}

/// The model and its ancestors.
pub(crate) struct Chain {
    /// The model first, then its parent, then the parent's parent.
    pub layers: Vec<Layer>,
    /// The chain ends in the `generated` family: the game extrudes the sprite of `layer0`.
    pub generated: bool,
}

/// Follows `parent` from the model until a model has none, a parent cannot be found, the chain is
/// too long or it loops. `path` is the model's own file, for the loop guard.
pub(crate) fn resolve(
    root: Value,
    folder: String,
    path: &str,
    files: &dyn TextureFiles,
    limits: &Limits,
    notices: &mut Vec<Notice>,
) -> Chain {
    let mut layers = vec![Layer {
        json: root,
        folder,
        builtin: false,
    }];
    let mut visited: HashSet<String> = HashSet::from([path.to_owned()]);
    let mut budget = limits.max_text;
    let mut generated = false;
    while let Some(parent) = layers
        .last()
        .and_then(|layer| text(&layer.json, "parent"))
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
    {
        if layers.len() > limits.max_parents as usize {
            notices.push(Notice::ParentChain { parent });
            break;
        }
        let id = parent.strip_prefix("minecraft:").unwrap_or(&parent);
        let builtin = builtin::lookup(id);
        if let Some(Builtin::Generated(json)) = builtin {
            generated = true;
            layers.push(Layer {
                json,
                folder: String::new(),
                builtin: true,
            });
            break;
        }
        let from = layers
            .last()
            .map(|layer| layer.folder.clone())
            .unwrap_or_default();
        match find_file(&parent, &from, files, limits, &mut budget, &mut visited) {
            Found::Model(json, folder) => {
                layers.push(Layer {
                    json,
                    folder,
                    builtin: false,
                });
                continue;
            }
            Found::Loop => {
                notices.push(Notice::ParentChain { parent });
                break;
            }
            Found::Unreadable => {
                notices.push(Notice::ParentUnreadable { parent });
                break;
            }
            Found::None => {}
        }
        match builtin {
            Some(Builtin::Model(json)) => layers.push(Layer {
                json,
                folder: String::new(),
                builtin: true,
            }),
            Some(Builtin::Unmerged) => {
                notices.push(Notice::ParentNotMerged { parent });
                break;
            }
            Some(Builtin::Generated(_)) => break,
            None => {
                notices.push(Notice::ParentNotFound { parent });
                break;
            }
        }
    }
    Chain { layers, generated }
}

enum Found {
    Model(Value, String),
    /// The file is already in the chain.
    Loop,
    /// A file exists but is not a model within the limits.
    Unreadable,
    None,
}

/// `dir/rest`, with no leading slash when `dir` is empty.
fn join(dir: &str, rest: &str) -> String {
    if dir.is_empty() {
        rest.to_owned()
    } else {
        format!("{dir}/{rest}")
    }
}

/// Looks for the parent's file: in the `assets/<ns>/models` tree that holds the model, in each
/// folder above the model, and next to it.
fn find_file(
    parent: &str,
    from: &str,
    files: &dyn TextureFiles,
    limits: &Limits,
    budget: &mut u64,
    visited: &mut HashSet<String>,
) -> Found {
    let (ns, path) = parent.split_once(':').unwrap_or(("minecraft", parent));
    if !safe_path(path) || ns.is_empty() || ns.contains('/') || !safe_path(ns) {
        return Found::None;
    }
    let dir = from.trim_end_matches('/');
    let mut candidates: Vec<String> = Vec::new();
    let assets = dir
        .rfind("/assets/")
        .and_then(|at| dir.get(..at))
        .or_else(|| dir.starts_with("assets/").then_some(""));
    if let Some(root) = assets {
        candidates.push(join(root, &format!("assets/{ns}/models/{path}.json")));
    }
    let mut above = dir;
    for _ in 0..8 {
        candidates.push(join(above, &format!("{path}.json")));
        match above.rfind('/').and_then(|at| above.get(..at)) {
            Some(next) => above = next,
            None => break,
        }
    }
    let last = path.rsplit('/').next().unwrap_or(path);
    candidates.push(join(dir, &format!("{last}.json")));

    for candidate in candidates {
        let Ok(Some(bytes)) = files.read(Path::new(&candidate)) else {
            continue;
        };
        if !visited.insert(candidate.clone()) {
            return Found::Loop;
        }
        let len = bytes.len() as u64;
        if len > *budget {
            return Found::Unreadable;
        }
        *budget -= len;
        let Ok(json) = parse(&bytes, limits) else {
            return Found::Unreadable;
        };
        let folder = match candidate.rfind('/').and_then(|at| candidate.get(..=at)) {
            Some(folder) => folder.to_owned(),
            None => String::new(),
        };
        return Found::Model(json, folder);
    }
    Found::None
}

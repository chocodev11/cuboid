//! The menu tables against the text of menus 1-3.

use std::collections::HashSet;

use super::*;
use crate::{ACTIONS, ActionKind, HandlerGroup, MenuId, spec};

/// The wording of the specification with the product names replaced (menus 2.8, 5.10).
fn branded(text: &str) -> String {
    if text == "BDcraft.net" {
        return "Project website".to_owned();
    }
    text.replace("BDcraft Cubik", "{App}")
        .replace("Cubik", "{App}")
}

fn icon_role(icon: &str) -> &str {
    match icon {
        "cubik_logo" => "app_logo",
        "bdcraft_logo" => "project_logo",
        other => other,
    }
}

fn spec_flags(letters: &str) -> RowFlags {
    letters
        .split_whitespace()
        .fold(RowFlags::empty(), |flags, letter| {
            flags
                | match letter {
                    "A" => RowFlags::ALWAYS_ACTIVE,
                    "U" => RowFlags::WORKS_IN_UI,
                    "L" => RowFlags::LINKED,
                    other => panic!("flag {other}"),
                }
        })
}

/// The number after "minWidth " in a heading or list line.
fn min_width(text: &str) -> u16 {
    let digits = text.split("minWidth ").nth(1).expect("a minWidth");
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().expect("digits")
}

/// The action name of a spec cell: "Rename (objects)" is the action Rename.
fn action_name(cell: &str) -> &str {
    cell.split(" (").next().unwrap_or(cell)
}

fn assert_bar_row(spec: &spec::Row, row: &RowDef, context: &str) {
    let label = spec
        .get("Label")
        .or_else(|| spec.get("Label (original)"))
        .expect("a label column");
    if label == "-" {
        assert_eq!(row.target, RowTarget::Separator, "{context}");
        return;
    }
    assert_eq!(row.label, branded(label), "{context}");
    assert_eq!(row.shortcut.unwrap_or(""), spec["Shortcut"], "{context}");
    assert_eq!(
        row.icon.unwrap_or(""),
        icon_role(&spec["Icon"]),
        "{context}"
    );
    let flags = spec.get("Flags").map_or("", String::as_str);
    assert_eq!(row.flags, spec_flags(flags), "{context}");
    let cell = &spec["Action"];
    match row.target {
        RowTarget::Action(id) => assert_eq!(id.def().name, action_name(cell), "{context}"),
        RowTarget::Child(child) => {
            assert!(cell.starts_with("child:"), "{context}");
            assert_eq!(child, MenuId::RecentFiles, "{context}");
        }
        other => panic!("{context}: {other:?}"),
    }
}

/// spec: menus 2.1-2.8
#[test]
fn the_bar_menus_equal_the_spec_tables() {
    let bar = [
        ("2.1", MenuId::File),
        ("2.2", MenuId::Edit),
        ("2.3", MenuId::Objects),
        ("2.4", MenuId::Recover),
        ("2.5", MenuId::Views),
        ("2.6", MenuId::Tools),
        ("2.7", MenuId::Window),
        ("2.8", MenuId::Help),
    ];
    for (number, id) in bar {
        let section = spec::section(number);
        let heading = section.lines().next().expect("a heading");
        let menu = id.def();
        assert_eq!(menu.min_width, min_width(heading), "{id:?}");
        // Tools starts disabled in the original; cuboid enables it (menus 2.6).
        let disabled = heading.contains("starts disabled") && id != MenuId::Tools;
        assert_eq!(menu.starts_disabled, disabled, "{id:?}");
        // The registration row is omitted (menus 2.8).
        let rows: Vec<_> = spec::table(section)
            .into_iter()
            .filter(|row| row["Action"] != "Registration")
            .collect();
        assert_eq!(rows.len(), menu.rows.len(), "{id:?}");
        for (index, (spec_row, row)) in rows.iter().zip(menu.rows).enumerate() {
            assert_bar_row(spec_row, row, &format!("{id:?} row {index}"));
        }
    }
}

fn handler(word: &str) -> Option<HandlerGroup> {
    match word {
        "elements" => Some(HandlerGroup::Elements),
        "meshes" => Some(HandlerGroup::Meshes),
        "objects" => Some(HandlerGroup::Objects),
        _ => None,
    }
}

/// The handler that the heading of a prose line names: "(minWidth 170, objects handler)".
fn heading_handler(line: &str) -> Option<HandlerGroup> {
    let (heading, _) = line.split_once("): ")?;
    let (_, rest) = heading.split_once("minWidth ")?.1.split_once(", ")?;
    handler(rest.split_whitespace().next()?)
}

/// Checks one item of a list that menus 3 writes as prose: `Label (Action, handler)`,
/// `Label (handler)` and `Label` for an action named like its label, `"Quoted label" (Action,
/// icon name)`, `Label (icon name, child Menu)` or `-`.
fn assert_item(
    item: &str,
    row: &RowDef,
    default_group: Option<HandlerGroup>,
    icons_apart: bool,
    context: &str,
) {
    if item == "-" {
        assert_eq!(row.target, RowTarget::Separator, "{context}");
        return;
    }
    let (label, notes) = match item.strip_prefix('"') {
        Some(quoted) => quoted
            .split_once("\" ")
            .unwrap_or((quoted.trim_end_matches('"'), "")),
        None => item.split_once(" (").unwrap_or((item, "")),
    };
    let context = format!("{context} {label}");
    assert_eq!(row.label, label, "{context}");
    let notes: Vec<_> = notes
        .trim_end_matches(')')
        .split(", ")
        .map(|note| note.trim_start_matches('('))
        .collect();
    let first = notes[0];
    // A line with an "Icons:" sentence lists its icons apart from the rows (checked separately).
    if let Some(icon) = notes.iter().find_map(|note| note.strip_prefix("icon ")) {
        assert_eq!(row.icon, Some(icon), "{context}");
    } else if !icons_apart {
        assert_eq!(row.icon, None, "{context}");
    }
    if first.starts_with("icon ") || first.starts_with("child") {
        assert!(matches!(row.target, RowTarget::Child(_)), "{context}");
        return;
    }
    let RowTarget::Action(id) = row.target else {
        panic!("{context}: {:?}", row.target);
    };
    let def = id.def();
    if first.starts_with("dead") {
        assert_eq!(def.kind, ActionKind::Dead, "{context}");
        return;
    }
    let (name, group) = match handler(first) {
        Some(group) => (label, Some(group)),
        None if first.is_empty() => (label, default_group),
        None => (
            first,
            notes
                .get(1)
                .and_then(|word| handler(word))
                .or(default_group),
        ),
    };
    assert_eq!((def.name, def.kind), (name, ActionKind::Live), "{context}");
    // Delete exists in the Edit and in the objects handler with the same behaviour (menus 5.3,
    // 5.4); the registry names the Edit handler.
    if let Some(group) = group.filter(|_| id != Id::Delete) {
        assert_eq!(def.group, group, "{context}");
    }
}

/// The popup lists that menus 3 writes as prose: menu, section, a marker of the line and the
/// text before the list on it.
const PROSE_LISTS: &[(MenuId, &str, &str, &str)] = &[
    (MenuId::Mesh, "3.1", "- **Mesh** (", "): "),
    (MenuId::MeshTransform, "3.1", "- **Mesh transform**", "): "),
    (MenuId::MeshTexture, "3.1", "- **Mesh texture**", "): "),
    (MenuId::MeshCopy, "3.1", "- **Mesh copy**", "): "),
    (MenuId::MeshPaste, "3.1", "- **Mesh paste**", "): "),
    (MenuId::Element, "3.2", "- **Element** (", "): "),
    (MenuId::Faces, "3.2", "- **Faces** (", "): "),
    (MenuId::Textures, "3.2", "- **Textures** (", "): "),
    (MenuId::UvMapping, "3.2", "- **UV Mapping**", "): "),
    (MenuId::Tint, "3.2", "- **Tint**", "): "),
    (MenuId::Culling, "3.2", "- **Culling**", "): "),
    (MenuId::Render, "3.2", "- **Render**", "): "),
    (MenuId::ElementCopy, "3.2", "- **Element copy**", "): "),
    (MenuId::ElementPaste, "3.2", "- **Element paste**", "): "),
    (MenuId::TexturePopup, "3.3", "Rows: ", "Rows: "),
    (MenuId::MaterialPopup, "3.4", "Rows: ", "Rows: "),
    (
        MenuId::NewTexturesList,
        "3.4",
        "New textures list (child",
        "): ",
    ),
    (MenuId::VoxelConvert, "3.6", "- **Voxel convert**", "): "),
    (
        MenuId::VoxelTransform,
        "3.6",
        "- **Voxel transform**",
        "): ",
    ),
    (
        MenuId::VoxelBlueprint,
        "3.6",
        "- **Voxel blueprint**",
        "): ",
    ),
    (MenuId::PalettePopup, "3.7", "\"RGB palette (default)\"", ""),
];

/// The list of a prose line without the sentences after it and the final full stop.
fn list_text(text: &str) -> &str {
    let list = text.split(". Icons").next().unwrap_or(text).trim_end();
    match list.strip_suffix('.') {
        Some(without) if without.ends_with(')') => without,
        _ => list,
    }
}

/// spec: menus 3.1-3.4, menus 3.6, menus 3.7
#[test]
fn the_popup_lists_equal_the_spec_prose() {
    for &(menu, number, marker, after) in PROSE_LISTS {
        let section = spec::section(number);
        let line = section
            .lines()
            .find(|line| line.contains(marker))
            .expect("a line");
        let def = menu.def();
        // Menus 3.3, 3.4 and 3.7 give the width in the heading.
        let width_text = if line.contains("minWidth") {
            line
        } else {
            section
        };
        assert_eq!(def.min_width, min_width(width_text), "{menu:?}");
        let items = spec::items(list_text(spec::line_after(section, marker, after)));
        assert_eq!(items.len(), def.rows.len(), "{menu:?}");
        let group = heading_handler(line);
        let icons_apart = line.contains("Icons: ") || line.contains("Icons as in");
        for (index, (item, row)) in items.iter().zip(def.rows).enumerate() {
            assert_item(
                item,
                row,
                group,
                icons_apart,
                &format!("{menu:?} row {index}"),
            );
        }
    }
}

/// spec: menus 3.1
#[test]
fn the_mesh_and_element_submenus_use_the_listed_icons() {
    let section = spec::section("3.1");
    let text = spec::line_after(section, "- **Mesh** (", "Icons: ");
    let listed: Vec<_> = text.trim_end_matches('.').split(", ").collect();
    // Menus 3.2: the Element submenu uses the icons of the Mesh submenu.
    for menu in [MenuId::Mesh, MenuId::Element] {
        let icons: Vec<_> = menu.def().rows.iter().filter_map(|row| row.icon).collect();
        assert_eq!(icons, listed, "{menu:?}");
    }
}

/// The top-level popups are tables in the specification.
///
/// spec: menus 3.1, menus 3.2, menus 3.6
#[test]
fn the_popup_tables_equal_the_spec_tables() {
    for (number, id) in [
        ("3.1", MenuId::MeshesPopup),
        ("3.2", MenuId::ElementsPopup),
        ("3.6", MenuId::VoxelPopup),
    ] {
        let section = spec::section(number);
        let heading = section.lines().next().expect("a heading");
        let menu = id.def();
        assert_eq!(menu.min_width, min_width(heading), "{id:?}");
        let rows = spec::table(section);
        assert_eq!(rows.len(), menu.rows.len(), "{id:?}");
        for (spec_row, row) in rows.iter().zip(menu.rows) {
            let label = spec_row["Label"].as_str();
            if label == "-" {
                assert_eq!(row.target, RowTarget::Separator, "{id:?}");
                continue;
            }
            assert_eq!(row.label, label, "{id:?}");
            assert_eq!(row.icon.unwrap_or(""), spec_row["Icon"], "{id:?} {label}");
            // The Elements popup lists only the names of its children.
            let cell = spec_row
                .get("Action / child")
                .or_else(|| spec_row.get("Child"))
                .expect("an action or child column");
            match row.target {
                RowTarget::Action(action) => {
                    assert_eq!(action.def().name, cell, "{id:?} {label}");
                }
                RowTarget::Child(_) => assert!(
                    spec_row.contains_key("Child") || cell.starts_with("child:"),
                    "{id:?} {label}"
                ),
                other => panic!("{id:?} {label}: {other:?}"),
            }
        }
    }
}

/// spec: menus 3.2
#[test]
fn the_face_lists_and_the_element_transform_follow_the_spec_sentences() {
    for (menu, up) in [
        (MenuId::FacesEnable, Id::FacesEnableUp),
        (MenuId::FacesDisable, Id::FacesDisableUp),
    ] {
        let def = menu.def();
        assert_eq!(def.min_width, 128);
        let labels: Vec<_> = def.rows.iter().map(|row| row.label).collect();
        assert_eq!(
            labels,
            [
                "Top", "Down", "-", "North", "South", "West", "East", "-", "All"
            ]
        );
        // "Top" is the Up face.
        assert_eq!(def.rows[0].target, RowTarget::Action(up));
    }
    let labels =
        |menu: MenuId| -> Vec<&str> { menu.def().rows.iter().map(|row| row.label).collect() };
    // Same rows as the Mesh transform, plus Inflate after Apply rescale.
    let mut expected = labels(MenuId::MeshTransform);
    let at = expected.iter().position(|&label| label == "Apply rescale");
    expected.insert(at.expect("a row") + 1, "Inflate...");
    assert_eq!(labels(MenuId::ElementTransform), expected);
    assert_eq!(MenuId::ElementTransform.def().min_width, 160);
}

/// spec: menus 3.8
#[test]
fn the_list_dropdowns_equal_the_spec_table() {
    let lists = [
        MenuId::ThemeList,
        MenuId::FaceCullingList,
        MenuId::UvFaceList,
        MenuId::PresetList,
        MenuId::TextureLabelList,
    ];
    let table = spec::table(spec::section("3.8"));
    assert_eq!(table.len(), lists.len());
    for (spec_row, menu) in table.iter().zip(lists) {
        let def = menu.def();
        assert_eq!(def.min_width.to_string(), spec_row["minWidth"], "{menu:?}");
        // Their rows have no actions. The preset and texture label lists are filled at run time.
        if matches!(menu, MenuId::PresetList | MenuId::TextureLabelList) {
            assert!(def.rows.is_empty());
            continue;
        }
        let labels: Vec<_> = def.rows.iter().map(|row| row.label).collect();
        assert_eq!(
            labels,
            spec_row["Rows"].split(", ").collect::<Vec<_>>(),
            "{menu:?}"
        );
        assert!(
            def.rows
                .iter()
                .all(|row| matches!(row.target, RowTarget::None | RowTarget::Separator))
        );
    }
}

/// spec: menus 1.7, menus 2.1
#[test]
fn the_dynamic_rows_have_the_spec_texts() {
    let recent = MenuId::RecentFiles.def();
    assert!(recent.starts_disabled);
    assert_eq!(recent.min_width, 196);
    assert_eq!(recent.rows.len(), 1);
    assert_eq!(recent.rows[0].label, "No recent files...");
    assert_eq!(RECENT_FILES_FOOTER[0].target, RowTarget::Separator);
    assert_eq!(RECENT_FILES_FOOTER[1].label, "Clear recent file list");
    assert_eq!(
        RECENT_FILES_FOOTER[1].target,
        RowTarget::Action(Id::ClearRecentFiles)
    );
    let files = spec::section("2.1");
    assert!(files.contains("\"No recent files...\"") && files.contains("Clear recent file list"));
    let dynamic = spec::section("1.7");
    assert!(dynamic.contains(&format!("\"{PASTE_OBJECTS}\"")));
    assert!(dynamic.contains(&format!("\"{PASTE_VOXELS}\"")));
}

#[test]
fn menus_follow_the_declaration_order_of_menu_id() {
    for (index, menu) in MENUS.iter().enumerate() {
        assert_eq!(menu.id as usize, index, "{:?}", menu.id);
    }
    // TextureLabelList is the last variant of MenuId.
    assert_eq!(MENUS.len(), MenuId::TextureLabelList as usize + 1);
}

/// spec: menus 1.1
#[test]
fn the_bar_menus_come_first_in_the_order_of_the_menu_bar() {
    let bar = spec::table(spec::section("1.1"));
    assert_eq!(bar.len(), 8);
    for (spec_row, menu) in bar.iter().zip(MENUS) {
        assert_eq!(format!("{:?}", menu.id), spec_row["Label"]);
    }
}

#[test]
fn separators_are_bare_and_every_submenu_hangs_below_one_row() {
    let mut children = Vec::new();
    for menu in MENUS {
        for row in menu.rows {
            let separator = row.target == RowTarget::Separator;
            assert_eq!(separator, row.label.starts_with('-'), "{}", row.label);
            if separator {
                assert!(row.shortcut.is_none() && row.icon.is_none() && row.flags.is_empty());
            }
            if let RowTarget::Child(child) = row.target {
                children.push(child);
            }
        }
    }
    let unique: HashSet<_> = children.iter().collect();
    assert_eq!(unique.len(), children.len());
    // Bar menus, popups and list dropdowns are opened by something other than a row.
    let opened_elsewhere = [
        MenuId::MeshesPopup,
        MenuId::ElementsPopup,
        MenuId::TexturePopup,
        MenuId::MaterialPopup,
        MenuId::VoxelPopup,
        MenuId::PalettePopup,
    ];
    for menu in MENUS {
        let index = menu.id as usize;
        let opened_by_row = index > MenuId::Help as usize && index < MenuId::ThemeList as usize;
        if opened_by_row && !opened_elsewhere.contains(&menu.id) {
            assert!(children.contains(&menu.id), "{:?} is unreachable", menu.id);
        }
    }
}

/// Every action is reachable from a row, a footer row, a hidden binding or a toolbar button.
#[test]
fn every_action_has_a_place() {
    let mut placed: HashSet<_> = MENUS
        .iter()
        .flat_map(|menu| menu.rows)
        .chain(&RECENT_FILES_FOOTER)
        .filter_map(|row| match row.target {
            RowTarget::Action(id) => Some(id),
            _ => None,
        })
        .collect();
    // The hidden Del binding (menus 5.3) and the "+" button of the material panel (menus 5.11).
    placed.extend([Id::DeleteConfirm, Id::AddTexture]);
    // The create buttons of the left tool strip (menus 5.14).
    placed.extend(ACTIONS.iter().map(|def| def.id).filter(|id| {
        let name = id.def().name;
        name.starts_with("CreateElement") || name.starts_with("CreateMesh")
    }));
    assert_eq!(placed.len(), ACTIONS.len());
}

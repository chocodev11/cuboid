//! The properties area: a vertical strip of icon tabs by context and the page of the chosen tab,
//! framed with a panel header.

use cuboid_ui_kit::metrics::height;
use cuboid_ui_kit::{Icon, PanelFrame, Tokens, Tone, icon_tab_badged};
use egui::{Align, Layout, Rect, Stroke, Ui, UiBuilder, pos2, vec2};

use crate::request::Mode;

/// The tabs of the properties area, in strip order.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub(crate) enum PropsTab {
    /// Name, transform and element settings.
    #[default]
    Element,
    /// Faces and UV of the element.
    Faces,
    /// The selected texture.
    Texture,
    /// Preset, model box, voxels, origin and blueprint.
    Model,
    /// The Minecraft display slots.
    Display,
    /// The pose of the selected bone and the selected keyframes.
    Animate,
    /// The findings of the model checker.
    Validator,
}

impl PropsTab {
    /// Every tab in strip order.
    pub const ALL: [PropsTab; 7] = [
        PropsTab::Element,
        PropsTab::Faces,
        PropsTab::Texture,
        PropsTab::Model,
        PropsTab::Display,
        PropsTab::Animate,
        PropsTab::Validator,
    ];

    /// Title of the tab and of its page.
    pub fn title(self) -> &'static str {
        match self {
            PropsTab::Element => "Element",
            PropsTab::Faces => "Faces and UV",
            PropsTab::Texture => "Texture",
            PropsTab::Model => "Model",
            PropsTab::Display => "Display",
            PropsTab::Animate => "Animate",
            PropsTab::Validator => "Validator",
        }
    }

    /// The tooltip and accessible name of the tab in the strip; it differs from the page title
    /// where the title is also the name of a workspace tab.
    pub fn tip(self) -> &'static str {
        match self {
            PropsTab::Element => "Element",
            PropsTab::Faces => "Faces and UV",
            PropsTab::Texture => "Texture",
            PropsTab::Model => "Model properties",
            PropsTab::Display => "Display properties",
            PropsTab::Animate => "Animate properties",
            PropsTab::Validator => "Validator",
        }
    }

    /// Icon of the tab.
    pub fn icon(self) -> Icon {
        match self {
            PropsTab::Element => Icon::Cube,
            PropsTab::Faces => Icon::FaceUv,
            PropsTab::Texture => Icon::Texture,
            PropsTab::Model => Icon::Package,
            PropsTab::Display => Icon::Display,
            PropsTab::Animate => Icon::Diamond,
            PropsTab::Validator => Icon::Validator,
        }
    }

    /// The tab a workspace puts in front: the model settings (voxels) in Voxel, the texture in
    /// Paint, the pose in Animate.
    pub fn for_mode(mode: Mode) -> PropsTab {
        match mode {
            Mode::Model => PropsTab::Element,
            Mode::Voxel => PropsTab::Model,
            Mode::Paint => PropsTab::Texture,
            Mode::Display => PropsTab::Display,
            Mode::Animate => PropsTab::Animate,
        }
    }
}

/// Draws the strip at one edge of the free space of `ui` and the page of `current` next to it. The
/// strip faces the viewports, so `strip_at` is `Align::Min` for the strip at the left of the page
/// (the area is right of the viewports) and `Align::Max` at its right. `badge` is the count on
/// the validator tab, `actions` fills the right end of the header and `page` the body. Returns the
/// tab that was clicked.
pub(crate) fn show(
    ui: &mut Ui,
    current: PropsTab,
    strip_at: Align,
    badge: Option<(usize, Tone)>,
    actions: impl FnOnce(&mut Ui),
    page: impl FnOnce(&mut Ui),
) -> Option<PropsTab> {
    let tokens = Tokens::of(ui.ctx());
    let area = ui.available_rect_before_wrap();
    let (strip, body) = if strip_at == Align::Max {
        (
            Rect::from_min_max(pos2(area.right() - height::STRIP, area.top()), area.max),
            Rect::from_min_max(area.min, pos2(area.right() - height::STRIP, area.bottom())),
        )
    } else {
        (
            Rect::from_min_size(area.min, vec2(height::STRIP, area.height())),
            Rect::from_min_max(pos2(area.left() + height::STRIP, area.top()), area.max),
        )
    };
    let mut clicked = None;

    let mut strip_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(strip)
            .layout(Layout::top_down(Align::Min)),
    );
    strip_ui.spacing_mut().item_spacing.y = 0.0;
    for tab in PropsTab::ALL {
        let count = (tab == PropsTab::Validator).then_some(badge).flatten();
        let response = icon_tab_badged(
            &mut strip_ui,
            tab.icon(),
            tab.tip(),
            tab == current,
            count,
            strip_at,
        );
        if response.clicked() {
            clicked = Some(tab);
        }
    }
    let edge = if strip_at == Align::Max {
        strip.left() + 0.5
    } else {
        strip.right() - 0.5
    };
    ui.painter()
        .vline(edge, strip.y_range(), Stroke::new(1.0, tokens.border));

    let mut body_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(body)
            .layout(Layout::top_down(Align::Min)),
    );
    PanelFrame::new(current.title())
        .icon(current.icon())
        .no_scroll()
        .show(&mut body_ui, actions, page);
    ui.advance_cursor_after_rect(area);
    clicked
}

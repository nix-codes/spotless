//! Browse's keys: Tab between the areas, and the arrows, Enter and
//! Insert in the results.

use egui::{FocusDirection, Id, Key, Ui};

use super::artist::{add_all_pressed, page_artist};
use super::search::{findable, searchable};
use super::{Act, Area, FIND_ID, PAGE_ROWS, SEARCH_ID, areas, shown};
use crate::action::{Action, Actions, BrowseAction};
use crate::ui::{Inputs, View};
use spotless_core::browse::{BrowseTab, Page};
use spotless_core::model::catalog::ReleaseTab;

/// Tab and Shift+Tab: the next or previous area. Into the search field
/// puts the keyboard there; out of it takes the keyboard away.
pub(super) fn switch_area(ui: &Ui, view: &View, inputs: &mut Inputs) {
    if inputs.tab == 0 {
        return;
    }
    let nav = &mut inputs.nav;
    nav.keyboard = true;
    // The search field is passed over while search isn't set up.
    let mut areas: Vec<Area> = areas(shown(view))
        .iter()
        .copied()
        .filter(|&a| a != Area::Search || searchable(view))
        .collect();
    // The find comes after the search field, while it's open.
    if view.browse.find.is_some() && shown(view) == BrowseTab::Search {
        let at = areas.iter().position(|&a| a == Area::Results).unwrap_or(0);
        areas.insert(at, Area::Find);
    }
    let at = areas.iter().position(|&a| a == nav.area).unwrap_or(0) as isize;
    let to = areas[(at + inputs.tab).rem_euclid(areas.len() as isize) as usize];
    inputs.tab = 0;
    enter_area(ui, inputs, to);
}

/// ← and →: the panel to the left or right, stopping at the ends. The
/// search field sits over the results, so it counts as them.
fn move_panel(ui: &Ui, view: &View, inputs: &mut Inputs, step: isize) {
    let panels: Vec<Area> = areas(shown(view))
        .iter()
        .copied()
        .filter(|&a| a != Area::Search)
        .collect();
    let area = match inputs.nav.area {
        Area::Search | Area::Find => Area::Results,
        area => area,
    };
    let at = panels.iter().position(|&a| a == area).unwrap_or(0) as isize;
    let to = (at + step).clamp(0, panels.len() as isize - 1) as usize;
    if panels[to] != inputs.nav.area {
        inputs.nav.keyboard = true;
        enter_area(ui, inputs, panels[to]);
    }
    // egui would also walk its focus sideways from a field given it.
    ui.ctx().memory_mut(|m| m.move_focus(FocusDirection::None));
}

/// Puts the keyboard in `area`: in its field, if it has one, out of the
/// others'.
pub(super) fn enter_area(ui: &Ui, inputs: &mut Inputs, area: Area) {
    let nav = &mut inputs.nav;
    nav.area = area;
    if nav.area == Area::Search {
        inputs.focus_search = true;
    } else {
        ui.ctx()
            .memory_mut(|m| m.surrender_focus(Id::new(SEARCH_ID)));
    }
    if nav.area == Area::Find {
        nav.focus_find = true;
    } else {
        ui.ctx().memory_mut(|m| m.surrender_focus(Id::new(FIND_ID)));
    }
    if nav.area == Area::Lists {
        nav.focus_filter = true;
    } else {
        ui.ctx()
            .memory_mut(|m| m.surrender_focus(Id::new(super::spotify::FILTER_ID)));
    }
    if nav.area == Area::Results && nav.row.is_none() {
        nav.row = Some(0);
    }
    nav.reveal = true;
}

pub(super) fn page_id(page: &Page) -> String {
    match page {
        Page::Home => "home".into(),
        Page::Search(query) => format!("search:{query}"),
        Page::Artist(uri) => uri.clone(),
    }
}

pub(super) fn keyboard(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    // The ADD TO list's keys are its own: Esc closes it, not Browse.
    if ui.ctx().text_edit_focused() || inputs.nav.box_open {
        return;
    }
    let searching = shown(view) == BrowseTab::Search;
    let artist_page = searching && matches!(view.browse.page(), Page::Artist(_));
    let to_field = searching && searchable(view);
    let finding = searching && view.browse.find.is_some();
    let findable = searching && findable(view);
    let nav = &mut inputs.nav;
    let focus_search = &mut inputs.focus_search;
    let mut up_to_field = false;
    let mut panel = 0;
    ui.input(|i| {
        let plain = i.modifiers.is_none();
        // Esc closes the find first, then Browse.
        if i.key_pressed(Key::Escape) {
            actions.push(if finding {
                Action::Browse(BrowseAction::Find(None))
            } else {
                Action::Browse(BrowseAction::CloseBrowse)
            });
        }
        if findable && !i.modifiers.command && !i.modifiers.alt && i.key_pressed(Key::Slash) {
            actions.push(BrowseAction::Find(Some(nav.find.clone())));
            nav.area = Area::Find;
            nav.focus_find = true;
            nav.keyboard = true;
        }
        let back = (i.modifiers.alt && i.key_pressed(Key::ArrowLeft))
            || (plain && i.key_pressed(Key::Backspace));
        if back && searching && view.browse.can_go_back() {
            actions.push(BrowseAction::Back);
        }
        if plain && i.key_pressed(Key::C) {
            actions.push(BrowseAction::ToggleCovers);
        }
        if artist_page {
            let numbers = [Key::Num1, Key::Num2, Key::Num3, Key::Num4];
            for (tab, key) in ReleaseTab::ALL.into_iter().zip(numbers) {
                if i.modifiers.alt && i.key_pressed(key) {
                    actions.push(BrowseAction::SetTab(tab));
                }
            }
            if i.modifiers.alt
                && i.key_pressed(Key::Num5)
                && let Some(artist) = page_artist(view)
            {
                actions.push(BrowseAction::ShowSimilar(artist));
            }
            if i.modifiers.alt && i.key_pressed(Key::Num6) {
                actions.push(BrowseAction::ShowPopular);
            }
            if i.modifiers.command && i.key_pressed(Key::S) {
                actions.push(BrowseAction::ToggleSort);
            }
            if i.modifiers.command && i.key_pressed(Key::E) {
                nav.expand_all = true;
            }
            if add_all_pressed(i) {
                nav.add_all = true;
            }
        }
        if plain && i.key_pressed(Key::ArrowLeft) {
            panel = -1;
        } else if plain && i.key_pressed(Key::ArrowRight) {
            panel = 1;
        }
        if nav.area != Area::Results || !plain {
            return;
        }
        // ↑ from the first row goes back up to the find, or the search
        // field.
        if (to_field || finding) && i.key_pressed(Key::ArrowUp) && nav.row.unwrap_or(0) == 0 {
            if finding {
                nav.area = Area::Find;
                nav.focus_find = true;
            } else {
                nav.area = Area::Search;
                *focus_search = true;
            }
            nav.row = None;
            nav.keyboard = true;
            up_to_field = true;
            return;
        }
        let last = nav.rows.saturating_sub(1);
        let from = nav.row.unwrap_or(0);
        let to = if i.key_pressed(Key::ArrowDown) {
            Some(if nav.row.is_some() { from + 1 } else { 0 })
        } else if i.key_pressed(Key::ArrowUp) {
            Some(from.saturating_sub(1))
        } else if i.key_pressed(Key::PageDown) {
            Some(from + PAGE_ROWS)
        } else if i.key_pressed(Key::PageUp) {
            Some(from.saturating_sub(PAGE_ROWS))
        } else if i.key_pressed(Key::Home) {
            Some(0)
        } else if i.key_pressed(Key::End) {
            Some(last)
        } else {
            None
        };
        if let Some(to) = to {
            nav.row = Some(to.min(last));
            nav.reveal = true;
            nav.keyboard = true;
        }
        if i.key_pressed(Key::Enter) {
            nav.act = Some(Act::Open);
        } else if i.key_pressed(Key::Insert) || i.key_pressed(Key::A) {
            nav.act = Some(Act::Add);
        }
    });
    // egui would also walk its focus up from the field, to the tabs.
    if up_to_field {
        ui.ctx().memory_mut(|m| m.move_focus(FocusDirection::None));
    }
    if panel != 0 {
        move_panel(ui, view, inputs, panel);
    }
}

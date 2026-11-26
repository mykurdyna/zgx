//! An element that takes the input method for text zgx doesn't edit itself:
//! a page another engine draws (an off-screen browser). While it has focus
//! and says its content is editable, it is the window's IME target, as a
//! WebKitGTK page is on Linux: compositions reach it as `ImePreedit` and
//! `ImeCommit`, and the candidates show at the caret it reports.
use std::cell::RefCell;
use zgx::scene::{NodeId, Rect};
use zgx::widgets::Ui;

#[derive(Clone, Copy)]
struct Page {
    node: NodeId,
    editing: bool,
    caret: Option<Rect>,
}

thread_local! {
    static PAGES: RefCell<Vec<Page>> = const { RefCell::new(Vec::new()) };
}

/// Say whether `node`'s content takes text now, and where its caret is, in
/// the window's points. Call it on the UI thread, then wake the window.
pub fn set(node: NodeId, editing: bool, caret: Option<Rect>) {
    PAGES.with(|pages| {
        let mut pages = pages.borrow_mut();
        match pages.iter_mut().find(|p| p.node == node) {
            Some(page) => {
                page.editing = editing;
                if caret.is_some() || !editing {
                    page.caret = caret;
                }
            }
            None => pages.push(Page {
                node,
                editing,
                caret,
            }),
        }
    });
}

/// Forget `node` (its element went away).
pub fn remove(node: NodeId) {
    PAGES.with(|pages| pages.borrow_mut().retain(|p| p.node != node));
}

fn focused_page(ui: &Ui) -> Option<Page> {
    let focused = ui.input.focused()?;
    PAGES.with(|pages| {
        pages
            .borrow()
            .iter()
            .copied()
            .find(|p| p.node == focused && p.editing)
    })
}

/// The focused page taking the input method, if any.
pub(crate) fn focused(ui: &Ui) -> Option<NodeId> {
    focused_page(ui).map(|p| p.node)
}

/// Where the focused page's caret shows, for the input method's candidates.
pub(crate) fn area(ui: &Ui) -> Option<Rect> {
    focused_page(ui).and_then(|p| p.caret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editable_page_context_follows_focus_and_removal() {
        let mut ui = Ui::new(400., 200.);
        let first = ui.button(ui.root(), "First", 100., || {});
        let second = ui.button(ui.root(), "Second", 100., || {});
        let shell = ui.button(ui.root(), "Shell", 100., || {});
        let first_caret = Rect::new(15., 30., 1., 18.);
        let second_caret = Rect::new(135., 30., 1., 18.);

        set(first, true, Some(first_caret));
        set(second, true, Some(second_caret));
        for (focus, expected, caret) in [
            (first, Some(first), Some(first_caret)),
            (shell, None, None),
            (second, Some(second), Some(second_caret)),
            (first, Some(first), Some(first_caret)),
        ] {
            ui.input.focus(&ui.scene, Some(focus));
            assert_eq!(focused(&ui), expected);
            assert_eq!(area(&ui), caret);
        }

        set(first, false, None);
        assert_eq!(focused(&ui), None);
        assert_eq!(area(&ui), None);
        ui.input.focus(&ui.scene, Some(second));
        remove(second);
        assert_eq!(focused(&ui), None);
        assert_eq!(area(&ui), None);
        remove(first);
    }
}

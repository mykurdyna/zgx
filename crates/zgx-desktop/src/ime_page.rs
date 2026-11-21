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
            None => pages.push(Page { node, editing, caret }),
        }
    });
}

/// Forget `node` (its element went away).
pub fn remove(node: NodeId) {
    PAGES.with(|pages| pages.borrow_mut().retain(|p| p.node != node));
}

fn focused_page(ui: &Ui) -> Option<Page> {
    let focused = ui.input.focused()?;
    PAGES.with(|pages| pages.borrow().iter().copied().find(|p| p.node == focused && p.editing))
}

/// The focused page taking the input method, if any.
pub(crate) fn focused(ui: &Ui) -> Option<NodeId> {
    focused_page(ui).map(|p| p.node)
}

/// Where the focused page's caret shows, for the input method's candidates.
pub(crate) fn area(ui: &Ui) -> Option<Rect> {
    focused_page(ui).and_then(|p| p.caret)
}

//! A frame of its own for an interface that takes itself for the whole
//! window.
//!
//! The general drawing interface was written as the root of the window: it
//! places its dropdowns, menus, key tips and dialogs by window coordinates and
//! by the window's size. Inside the shell it is not at the window's top-left
//! corner any more. Rather than teach every one of those places an offset,
//! [`Rebase`] gives the interface what it assumes:
//!
//! * it is laid out with its top-left corner at `(0, 0)`, in the size of the
//!   area it is given (the application tells it that size as its "window
//!   size", see `PidShell::editor_frame`);
//! * it is drawn translated to where the area is, and clipped to it;
//! * the pointer, and the position inside mouse and touch events, reach it
//!   with the area's origin taken off;
//! * its overlays (tooltips, pick lists, context menus) get the origin as
//!   their translation, which puts them back in window coordinates;
//! * the bounds its widgets record stay in its own frame, and
//!   `ui::wrap_bar` adds the origin for whoever asks where a widget is in
//!   the window (`ui_bounds`, cropping a screenshot to the drawing area).
//!
//! What it does not do: a modal dialog of the interface dims the area, not
//! the window, so the shell around it stays live while the dialog is open.

use std::borrow::Cow;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, InputMethod, Renderer as _, Shell};
use iced::{touch, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use crate::app::Message;

/// `child`, laid out and addressed in a frame of its own (see the module
/// docs). It fills the area it is put in.
pub struct Rebase<'a> {
    child: Element<'a, Message>,
    /// The child's layout in its own frame, top-left corner at `(0, 0)`.
    /// Kept from `layout`: the `Layout` the parent hands down can only say
    /// where the child is in the window.
    frame: layout::Node,
}

impl<'a> Rebase<'a> {
    /// Put `child` in a frame of its own.
    pub fn new(child: impl Into<Element<'a, Message>>) -> Self {
        Self {
            child: child.into(),
            frame: layout::Node::default(),
        }
    }
}

/// Where the frame starts in the window.
fn origin(layout: Layout<'_>) -> Vector {
    let bounds = layout.bounds();
    Vector::new(bounds.x, bounds.y)
}

/// `event` as the frame sees it: the positions it carries are the window's,
/// and the frame's are `origin` less.
fn in_frame(event: &Event, origin: Vector) -> Cow<'_, Event> {
    match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Cow::Owned(Event::Mouse(mouse::Event::CursorMoved {
                position: *position - origin,
            }))
        }
        Event::Touch(event) => Cow::Owned(Event::Touch(match *event {
            touch::Event::FingerPressed { id, position } => touch::Event::FingerPressed {
                id,
                position: position - origin,
            },
            touch::Event::FingerMoved { id, position } => touch::Event::FingerMoved {
                id,
                position: position - origin,
            },
            touch::Event::FingerLifted { id, position } => touch::Event::FingerLifted {
                id,
                position: position - origin,
            },
            touch::Event::FingerLost { id, position } => touch::Event::FingerLost {
                id,
                position: position - origin,
            },
        })),
        other => Cow::Borrowed(other),
    }
}

impl<'a> Widget<Message, Theme, Renderer> for Rebase<'a> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.child));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let area = limits.max();
        self.frame = self.child.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, area),
        );
        layout::Node::new(area)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let origin = origin(layout);
        self.child.as_widget_mut().update(
            &mut tree.children[0],
            &in_frame(event, origin),
            Layout::new(&self.frame),
            cursor - origin,
            renderer,
            shell,
            &(*viewport - origin),
        );
        // The caret an input method is asked to stay clear of is where the
        // window has it.
        if let InputMethod::Enabled { cursor, .. } = shell.input_method_mut() {
            *cursor = *cursor + origin;
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let origin = origin(layout);
        self.child.as_widget().mouse_interaction(
            &tree.children[0],
            Layout::new(&self.frame),
            cursor - origin,
            &(*viewport - origin),
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        _layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.child.as_widget_mut().operate(
            &mut tree.children[0],
            Layout::new(&self.frame),
            renderer,
            operation,
        );
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let origin = origin(layout);
        let area = layout.bounds();
        crate::ui::wrap_bar::with_origin(origin, || {
            renderer.with_layer(area, |renderer| {
                renderer.with_translation(origin, |renderer| {
                    self.child.as_widget().draw(
                        &tree.children[0],
                        renderer,
                        theme,
                        style,
                        Layout::new(&self.frame),
                        cursor - origin,
                        &(*viewport - origin),
                    );
                });
            });
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let origin = origin(layout);
        let Self { child, frame } = self;
        child.as_widget_mut().overlay(
            &mut tree.children[0],
            Layout::new(frame),
            renderer,
            &(*viewport - origin),
            translation + origin,
        )
    }
}

impl<'a> From<Rebase<'a>> for Element<'a, Message> {
    fn from(rebase: Rebase<'a>) -> Self {
        Element::new(rebase)
    }
}

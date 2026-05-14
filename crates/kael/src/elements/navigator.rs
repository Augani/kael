use crate::{
    AnyElement, AnyView, Context, EventEmitter, IntoElement, ParentElement, Render, SharedString,
    StyleRefinement, Styled, Window, div, relative,
};
use std::{
    any::Any,
    rc::Rc,
    time::{Duration, Instant},
};

const SLIDE_TRANSITION_DURATION: Duration = Duration::from_millis(220);
const FADE_TRANSITION_DURATION: Duration = Duration::from_millis(180);

/// Creates a navigator initialized with a single route.
pub fn navigator(initial_route: impl Into<Route>) -> Navigator {
    Navigator::new(initial_route)
}

/// A route rendered by a [`Navigator`].
pub struct Route {
    id: SharedString,
    view: AnyView,
    memento: Option<Box<dyn Any>>,
}

impl Route {
    /// Creates a route for the given view.
    pub fn new(id: impl Into<SharedString>, view: impl Into<AnyView>) -> Self {
        Self {
            id: id.into(),
            view: view.into(),
            memento: None,
        }
    }

    /// Returns this route's identifier.
    pub fn id(&self) -> &SharedString {
        &self.id
    }

    /// Returns the view rendered for this route.
    pub fn view(&self) -> AnyView {
        self.view.clone()
    }

    /// Attaches restorable state to this route.
    pub fn with_memento<T: Any>(mut self, memento: T) -> Self {
        self.memento = Some(Box::new(memento));
        self
    }

    /// Returns a shared reference to the stored memento if it matches `T`.
    pub fn memento<T: Any>(&self) -> Option<&T> {
        self.memento
            .as_deref()
            .and_then(|memento| memento.downcast_ref())
    }

    /// Removes and returns the stored memento if it matches `T`.
    pub fn take_memento<T: Any>(&mut self) -> Option<T> {
        let memento = self.memento.take()?;
        memento.downcast::<T>().ok().map(|memento| *memento)
    }
}

/// An event emitted whenever the active route changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteChangeEvent {
    /// The route that was active before the change, if any.
    pub previous_route_id: Option<SharedString>,
    /// The route that is active after the change, if any.
    pub current_route_id: Option<SharedString>,
    /// The number of routes currently on the stack.
    pub stack_depth: usize,
}

/// A transition applied between navigation stack changes.
#[derive(Clone)]
pub enum Transition {
    /// Switch routes immediately without animation.
    None,
    /// Slide the outgoing route left while the incoming route enters from the right.
    SlideLeft,
    /// Slide the outgoing route right while the incoming route enters from the left.
    SlideRight,
    /// Slide the outgoing route up while the incoming route enters from the bottom.
    SlideUp,
    /// Slide the outgoing route down while the incoming route enters from the top.
    SlideDown,
    /// Cross-fade between routes.
    Fade,
    /// Use a custom animator to render the transition frame.
    Custom(Rc<dyn TransitionAnimator>),
}

impl Transition {
    /// Returns the total duration of the transition.
    pub fn duration(&self) -> Duration {
        match self {
            Self::None => Duration::ZERO,
            Self::SlideLeft | Self::SlideRight | Self::SlideUp | Self::SlideDown => {
                SLIDE_TRANSITION_DURATION
            }
            Self::Fade => FADE_TRANSITION_DURATION,
            Self::Custom(animator) => animator.duration(),
        }
    }
}

/// Renders a custom navigation transition.
pub trait TransitionAnimator: 'static {
    /// Returns the duration used by the custom transition.
    fn duration(&self) -> Duration;

    /// Renders a single transition frame for the given progress.
    fn render_frame(&self, progress: f32, outgoing: AnyView, incoming: AnyView) -> AnyElement;
}

struct ActiveTransition {
    transition: Transition,
    started_at: Instant,
    outgoing: AnyView,
    incoming: AnyView,
}

impl ActiveTransition {
    fn new(transition: Transition, outgoing: AnyView, incoming: AnyView) -> Self {
        Self {
            transition,
            started_at: Instant::now(),
            outgoing,
            incoming,
        }
    }

    fn progress(&self, animations_enabled: bool) -> (f32, bool) {
        if !animations_enabled {
            return (1.0, true);
        }

        let duration = self.transition.duration();
        if duration.is_zero() {
            return (1.0, true);
        }

        let elapsed = self.started_at.elapsed();
        let progress = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
        (progress, progress >= 1.0)
    }
}

struct NavigationChange {
    previous_route_id: Option<SharedString>,
    current_route_id: Option<SharedString>,
}

/// A renderable navigation stack that supports animated route transitions.
pub struct Navigator {
    stack: Vec<Route>,
    transition: Option<ActiveTransition>,
}

impl Navigator {
    /// Creates an empty navigator.
    pub fn empty() -> Self {
        Self {
            stack: Vec::new(),
            transition: None,
        }
    }

    /// Creates a navigator with an initial route.
    pub fn new(initial_route: impl Into<Route>) -> Self {
        Self {
            stack: vec![initial_route.into()],
            transition: None,
        }
    }

    /// Returns the number of routes on the stack.
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Returns whether the navigator has no routes.
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Returns the currently visible route.
    pub fn current_route(&self) -> Option<&Route> {
        self.stack.last()
    }

    /// Returns the current route stack from root to top.
    pub fn routes(&self) -> &[Route] {
        &self.stack
    }

    /// Returns the identifier of the currently visible route.
    pub fn current_route_id(&self) -> Option<&SharedString> {
        self.current_route().map(Route::id)
    }

    /// Pushes a new route onto the stack.
    pub fn push(
        &mut self,
        route: impl Into<Route>,
        transition: Transition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let change = self.push_route(route.into(), transition);
        self.finish_change(change, window, cx);
    }

    /// Pops the top route from the stack.
    pub fn pop(
        &mut self,
        transition: Transition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Route> {
        let (route, change) = self.pop_route(transition)?;
        self.finish_change(change, window, cx);
        Some(route)
    }

    /// Replaces the current route with a new route.
    pub fn replace(
        &mut self,
        route: impl Into<Route>,
        transition: Transition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let change = self.replace_route(route.into(), transition);
        self.finish_change(change, window, cx);
    }

    /// Replaces the full route stack atomically.
    pub fn replace_stack(
        &mut self,
        routes: impl IntoIterator<Item = Route>,
        transition: Transition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let change = self.replace_stack_routes(routes.into_iter().collect(), transition);
        self.finish_change(change, window, cx);
    }

    /// Pops the stack back to the first route.
    pub fn pop_to_root(
        &mut self,
        transition: Transition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(change) = self.pop_to_root_routes(transition) else {
            return;
        };
        self.finish_change(change, window, cx);
    }

    fn finish_change(
        &mut self,
        change: NavigationChange,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(RouteChangeEvent {
            previous_route_id: change.previous_route_id,
            current_route_id: change.current_route_id,
            stack_depth: self.stack.len(),
        });
        cx.notify();
    }

    fn push_route(&mut self, route: Route, transition: Transition) -> NavigationChange {
        let previous_route_id = self.current_route_id().cloned();
        let outgoing = self.current_view();
        self.stack.push(route);
        let current_route_id = self.current_route_id().cloned();
        let incoming = self.current_view();
        self.begin_transition(transition, outgoing, incoming);
        NavigationChange {
            previous_route_id,
            current_route_id,
        }
    }

    fn pop_route(&mut self, transition: Transition) -> Option<(Route, NavigationChange)> {
        if self.stack.is_empty() {
            return None;
        }

        let previous_route_id = self.current_route_id().cloned();
        let outgoing = self.current_view();
        let route = self.stack.pop().expect("checked stack is non-empty");
        let current_route_id = self.current_route_id().cloned();
        let incoming = self.current_view();
        self.begin_transition(transition, outgoing, incoming);

        Some((
            route,
            NavigationChange {
                previous_route_id,
                current_route_id,
            },
        ))
    }

    fn replace_route(&mut self, route: Route, transition: Transition) -> NavigationChange {
        let previous_route_id = self.current_route_id().cloned();
        let outgoing = self.current_view();
        if let Some(current) = self.stack.last_mut() {
            *current = route;
        } else {
            self.stack.push(route);
        }
        let current_route_id = self.current_route_id().cloned();
        let incoming = self.current_view();
        self.begin_transition(transition, outgoing, incoming);

        NavigationChange {
            previous_route_id,
            current_route_id,
        }
    }

    fn replace_stack_routes(
        &mut self,
        routes: Vec<Route>,
        transition: Transition,
    ) -> NavigationChange {
        let previous_route_id = self.current_route_id().cloned();
        let outgoing = self.current_view();
        self.stack = routes;
        let current_route_id = self.current_route_id().cloned();
        let incoming = self.current_view();
        self.begin_transition(transition, outgoing, incoming);

        NavigationChange {
            previous_route_id,
            current_route_id,
        }
    }

    fn pop_to_root_routes(&mut self, transition: Transition) -> Option<NavigationChange> {
        if self.stack.len() <= 1 {
            return None;
        }

        let previous_route_id = self.current_route_id().cloned();
        let outgoing = self.current_view();
        let root = self.stack.drain(..1).next().expect("root route exists");
        self.stack.clear();
        self.stack.push(root);
        let current_route_id = self.current_route_id().cloned();
        let incoming = self.current_view();
        self.begin_transition(transition, outgoing, incoming);

        Some(NavigationChange {
            previous_route_id,
            current_route_id,
        })
    }

    fn begin_transition(
        &mut self,
        transition: Transition,
        outgoing: Option<AnyView>,
        incoming: Option<AnyView>,
    ) {
        self.transition = None;
        if matches!(transition, Transition::None) {
            return;
        }

        let (Some(outgoing), Some(incoming)) = (outgoing, incoming) else {
            return;
        };
        if outgoing == incoming {
            return;
        }

        self.transition = Some(ActiveTransition::new(transition, outgoing, incoming));
    }

    fn current_view(&self) -> Option<AnyView> {
        self.current_route().map(Route::view)
    }
}

impl EventEmitter<RouteChangeEvent> for Navigator {}

impl Render for Navigator {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div().relative().w_full().h_full().overflow_hidden();

        if let Some((transition, progress, finished, outgoing, incoming)) =
            self.transition.as_ref().map(|active_transition| {
                let (progress, finished) = active_transition.progress(window.animations_enabled());
                (
                    active_transition.transition.clone(),
                    progress,
                    finished,
                    active_transition.outgoing.clone(),
                    active_transition.incoming.clone(),
                )
            })
        {
            if finished {
                self.transition = None;
            } else {
                window.request_animation_frame();
                root = root.child(render_transition_frame(
                    transition, progress, outgoing, incoming,
                ));
                return root;
            }
        }

        if let Some(route) = self.current_route() {
            root = root.child(fill_view(route.view()));
        }

        root
    }
}

fn render_transition_frame(
    transition: Transition,
    progress: f32,
    outgoing: AnyView,
    incoming: AnyView,
) -> AnyElement {
    match transition {
        Transition::None => fill_view(incoming),
        Transition::SlideLeft => horizontal_slide(progress, outgoing, incoming, -progress),
        Transition::SlideRight => horizontal_slide(progress, incoming, outgoing, progress - 1.0),
        Transition::SlideUp => vertical_slide(progress, outgoing, incoming, -progress),
        Transition::SlideDown => vertical_slide(progress, incoming, outgoing, progress - 1.0),
        Transition::Fade => div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h_full()
                    .opacity(1.0 - progress)
                    .child(cached_view(outgoing)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h_full()
                    .opacity(progress)
                    .child(cached_view(incoming)),
            )
            .into_any_element(),
        Transition::Custom(animator) => animator.render_frame(progress, outgoing, incoming),
    }
}

fn horizontal_slide(progress: f32, leading: AnyView, trailing: AnyView, left: f32) -> AnyElement {
    let _ = progress;
    div()
        .absolute()
        .top_0()
        .left(relative(left))
        .w(relative(2.0))
        .h_full()
        .flex()
        .flex_row()
        .child(
            div()
                .w_full()
                .h_full()
                .flex_none()
                .child(cached_view(leading)),
        )
        .child(
            div()
                .w_full()
                .h_full()
                .flex_none()
                .child(cached_view(trailing)),
        )
        .into_any_element()
}

fn vertical_slide(progress: f32, leading: AnyView, trailing: AnyView, top: f32) -> AnyElement {
    let _ = progress;
    div()
        .absolute()
        .top(relative(top))
        .left_0()
        .w_full()
        .h(relative(2.0))
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .h_full()
                .flex_none()
                .child(cached_view(leading)),
        )
        .child(
            div()
                .w_full()
                .h_full()
                .flex_none()
                .child(cached_view(trailing)),
        )
        .into_any_element()
}

fn fill_view(view: AnyView) -> AnyElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .w_full()
        .h_full()
        .child(view)
        .into_any_element()
}

fn cached_view(view: AnyView) -> AnyElement {
    view.cached(StyleRefinement::default()).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Navigator, Route, Transition, navigator};
    use crate::{self as gpui, AppContext, EmptyView, TestAppContext};

    #[kael::test]
    fn navigator_updates_stack_for_push_replace_pop_and_root(cx: &mut TestAppContext) {
        let (navigator_view, mut window) =
            cx.add_window_view(|_, cx| navigator(Route::new("home", cx.new(|_| EmptyView))));

        window.update(|window, cx| {
            navigator_view.update(cx, |navigator, cx| {
                navigator.push(
                    Route::new("settings", cx.new(|_| EmptyView)),
                    Transition::None,
                    window,
                    cx,
                );
                navigator.push(
                    Route::new("details", cx.new(|_| EmptyView)),
                    Transition::None,
                    window,
                    cx,
                );
                assert_eq!(
                    navigator
                        .stack
                        .iter()
                        .map(|route| route.id.as_ref())
                        .collect::<Vec<_>>(),
                    vec!["home", "settings", "details"]
                );

                let popped = navigator
                    .pop(Transition::None, window, cx)
                    .expect("details route should pop");
                assert_eq!(popped.id.as_ref(), "details");

                navigator.replace(
                    Route::new("profile", cx.new(|_| EmptyView)),
                    Transition::None,
                    window,
                    cx,
                );
                assert_eq!(
                    navigator.current_route_id().map(|id| id.as_ref()),
                    Some("profile")
                );

                navigator.pop_to_root(Transition::None, window, cx);
                assert_eq!(
                    navigator
                        .stack
                        .iter()
                        .map(|route| route.id.as_ref())
                        .collect::<Vec<_>>(),
                    vec!["home"]
                );

                let root = navigator
                    .pop(Transition::None, window, cx)
                    .expect("root should pop");
                assert_eq!(root.id.as_ref(), "home");
                assert!(navigator.is_empty());
            });
        });
    }

    #[kael::test]
    fn navigator_creates_transition_for_animated_changes(cx: &mut TestAppContext) {
        let (navigator_view, mut window) =
            cx.add_window_view(|_, cx| Navigator::new(Route::new("home", cx.new(|_| EmptyView))));

        window.update(|window, cx| {
            navigator_view.update(cx, |navigator, cx| {
                navigator.push(
                    Route::new("settings", cx.new(|_| EmptyView)),
                    Transition::SlideLeft,
                    window,
                    cx,
                );
                assert!(navigator.transition.is_some());
            });
        });
    }

    #[kael::test]
    fn navigator_replaces_and_exposes_route_stack(cx: &mut TestAppContext) {
        let (navigator_view, mut window) =
            cx.add_window_view(|_, cx| Navigator::new(Route::new("home", cx.new(|_| EmptyView))));

        window.update(|window, cx| {
            navigator_view.update(cx, |navigator, cx| {
                navigator.replace_stack(
                    vec![
                        Route::new("home", cx.new(|_| EmptyView)).with_memento(1usize),
                        Route::new("thread", cx.new(|_| EmptyView))
                            .with_memento(String::from("inbox/42")),
                    ],
                    Transition::None,
                    window,
                    cx,
                );

                assert_eq!(
                    navigator
                        .routes()
                        .iter()
                        .map(|route| route.id().as_ref())
                        .collect::<Vec<_>>(),
                    vec!["home", "thread"]
                );
                assert_eq!(
                    navigator.current_route_id().map(|route| route.as_ref()),
                    Some("thread")
                );
                assert_eq!(navigator.routes()[0].memento::<usize>(), Some(&1usize));
                assert_eq!(
                    navigator.routes()[1]
                        .memento::<String>()
                        .map(String::as_str),
                    Some("inbox/42")
                );

                navigator.replace_stack(Vec::new(), Transition::None, window, cx);

                assert!(navigator.routes().is_empty());
                assert!(navigator.current_route().is_none());
            });
        });
    }
}

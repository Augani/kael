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

    /// Byte length of the route identifier, without exposing the identifier.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Returns true when this route has restorable state attached.
    pub fn has_memento(&self) -> bool {
        self.memento.is_some()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "route(id_len_bytes={}, has_memento={})",
            self.id_len_bytes(),
            self.has_memento()
        )
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

impl RouteChangeEvent {
    /// Returns true when the event has a previous route.
    pub fn has_previous_route(&self) -> bool {
        self.previous_route_id.is_some()
    }

    /// Returns true when the event has a current route.
    pub fn has_current_route(&self) -> bool {
        self.current_route_id.is_some()
    }

    /// Byte length of the previous route identifier, without exposing it.
    pub fn previous_route_id_len_bytes(&self) -> usize {
        self.previous_route_id
            .as_ref()
            .map_or(0, |route_id| route_id.len())
    }

    /// Byte length of the current route identifier, without exposing it.
    pub fn current_route_id_len_bytes(&self) -> usize {
        self.current_route_id
            .as_ref()
            .map_or(0, |route_id| route_id.len())
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "route_change_event(has_previous_route={}, previous_route_id_len_bytes={}, has_current_route={}, current_route_id_len_bytes={}, stack_depth={})",
            self.has_previous_route(),
            self.previous_route_id_len_bytes(),
            self.has_current_route(),
            self.current_route_id_len_bytes(),
            self.stack_depth
        )
    }
}

/// Checked summary of one app-owned navigation route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationRouteDescriptor {
    id: SharedString,
    has_restorable_state: bool,
    requires_activation: bool,
}

impl NavigationRouteDescriptor {
    /// Start building a route descriptor for an app-owned route id.
    pub fn builder(id: impl Into<SharedString>) -> NavigationRouteDescriptorBuilder {
        NavigationRouteDescriptorBuilder::new(id)
    }

    /// App-owned route id.
    pub fn id(&self) -> &SharedString {
        &self.id
    }

    /// Byte length of the route id, without exposing it in summaries.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Whether this route carries restorable state.
    pub fn has_restorable_state(&self) -> bool {
        self.has_restorable_state
    }

    /// Whether this route requires app activation before navigation.
    pub fn requires_activation(&self) -> bool {
        self.requires_activation
    }

    /// Validate route metadata before it becomes part of a navigation handoff.
    pub fn validate(&self) -> Result<(), SharedString> {
        validate_navigation_route_id(&self.id)?;
        Ok(())
    }

    /// Content-safe summary for agents, tests, and logs.
    pub fn to_text(&self) -> String {
        format!(
            "navigation_route(id_len_bytes={}, restorable={}, requires_activation={})",
            self.id_len_bytes(),
            self.has_restorable_state(),
            self.requires_activation()
        )
    }
}

/// Builder for checked route descriptors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationRouteDescriptorBuilder {
    id: SharedString,
    has_restorable_state: bool,
    requires_activation: bool,
}

impl NavigationRouteDescriptorBuilder {
    /// Create a route descriptor builder.
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            has_restorable_state: false,
            requires_activation: false,
        }
    }

    /// Mark the route as carrying restorable state.
    pub fn restorable_state(mut self) -> Self {
        self.has_restorable_state = true;
        self
    }

    /// Mark the route as requiring app activation before navigation.
    pub fn require_activation(mut self) -> Self {
        self.requires_activation = true;
        self
    }

    /// Validate the route descriptor shape.
    pub fn validate(&self) -> Result<(), SharedString> {
        self.as_descriptor()?.validate()
    }

    /// Build a checked route descriptor.
    pub fn build_checked(self) -> Result<NavigationRouteDescriptor, SharedString> {
        let descriptor = self.as_descriptor()?;
        descriptor.validate()?;
        Ok(descriptor)
    }

    fn as_descriptor(&self) -> Result<NavigationRouteDescriptor, SharedString> {
        Ok(NavigationRouteDescriptor {
            id: self.id.clone(),
            has_restorable_state: self.has_restorable_state,
            requires_activation: self.requires_activation,
        })
    }
}

/// Unit of work covered by a checked navigation handoff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationHandoffRequest {
    /// Validate or scaffold one app-owned native route.
    Route(NavigationRouteDescriptor),
    /// Push a route onto the native stack.
    PushRoute {
        /// Target route id.
        route_id: SharedString,
    },
    /// Replace the current route with another route.
    ReplaceRoute {
        /// Target route id.
        route_id: SharedString,
    },
    /// Pop one route from the native stack.
    PopRoute,
    /// Pop the native stack back to the root route.
    PopToRoot,
    /// Restore a native navigation stack from a session snapshot.
    RestoreSession {
        /// Number of routes expected in the restored stack.
        stack_depth: usize,
    },
    /// Route an incoming deep link into native navigation.
    DeepLink {
        /// URL scheme handled by this route.
        scheme: SharedString,
    },
    /// Delegate browser-owned history to an explicit hosted surface.
    HostedHistoryBridge {
        /// Hosted surface id.
        surface_id: SharedString,
    },
}

impl NavigationHandoffRequest {
    /// Validate one navigation handoff request.
    pub fn validate(&self) -> Result<(), SharedString> {
        match self {
            Self::Route(route) => route.validate(),
            Self::PushRoute { route_id } | Self::ReplaceRoute { route_id } => {
                validate_navigation_route_id(route_id)
            }
            Self::PopRoute | Self::PopToRoot => Ok(()),
            Self::RestoreSession { stack_depth } => {
                if *stack_depth == 0 || *stack_depth > 128 {
                    return Err("navigation restore stack depth must be between 1 and 128".into());
                }
                Ok(())
            }
            Self::DeepLink { scheme } => validate_navigation_scheme(scheme),
            Self::HostedHistoryBridge { surface_id } => validate_navigation_surface_id(surface_id),
        }
    }

    /// Privacy-preserving request kind for handoff summaries.
    pub fn summary_kind(&self) -> &'static str {
        match self {
            Self::Route(_) => "route",
            Self::PushRoute { .. } => "push-route",
            Self::ReplaceRoute { .. } => "replace-route",
            Self::PopRoute => "pop-route",
            Self::PopToRoot => "pop-to-root",
            Self::RestoreSession { .. } => "restore-session",
            Self::DeepLink { .. } => "deep-link",
            Self::HostedHistoryBridge { .. } => "hosted-history-bridge",
        }
    }
}

/// Recommended next implementation action for a navigation handoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationHandoffNextAction {
    /// Validate route descriptors before rendering navigation UI.
    ValidateNativeRoutes,
    /// Restore the route stack from a session snapshot.
    RestoreSessionStack,
    /// Route an incoming deep link.
    HandleDeepLink,
    /// Apply native stack commands.
    ApplyNativeNavigationCommand,
    /// Use a hosted browser-history bridge.
    UseHostedHistoryBridge,
}

/// Checked handoff for native navigation, history, routing, restore, and hosted fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationHandoff {
    requests: Vec<NavigationHandoffRequest>,
}

impl NavigationHandoff {
    /// Start building a navigation handoff.
    pub fn builder() -> NavigationHandoffBuilder {
        NavigationHandoffBuilder::new()
    }

    /// Requests covered by this handoff.
    pub fn requests(&self) -> &[NavigationHandoffRequest] {
        &self.requests
    }

    /// Number of requests in the handoff.
    pub fn request_count(&self) -> usize {
        self.requests.len()
    }

    /// Whether this handoff includes native route descriptors.
    pub fn has_native_routes(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, NavigationHandoffRequest::Route(_)))
    }

    /// Whether this handoff includes session restore.
    pub fn has_session_restore(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, NavigationHandoffRequest::RestoreSession { .. }))
    }

    /// Whether this handoff includes deep-link routing.
    pub fn has_deep_link(&self) -> bool {
        self.requests
            .iter()
            .any(|request| matches!(request, NavigationHandoffRequest::DeepLink { .. }))
    }

    /// Whether this handoff includes hosted browser history fallback.
    pub fn has_hosted_history_bridge(&self) -> bool {
        self.requests.iter().any(|request| {
            matches!(
                request,
                NavigationHandoffRequest::HostedHistoryBridge { .. }
            )
        })
    }

    /// First recommended action for a builder or AI agent.
    pub fn next_action(&self) -> NavigationHandoffNextAction {
        if self.has_native_routes() {
            NavigationHandoffNextAction::ValidateNativeRoutes
        } else if self.has_session_restore() {
            NavigationHandoffNextAction::RestoreSessionStack
        } else if self.has_deep_link() {
            NavigationHandoffNextAction::HandleDeepLink
        } else if self.requests.iter().any(|request| {
            matches!(
                request,
                NavigationHandoffRequest::PushRoute { .. }
                    | NavigationHandoffRequest::ReplaceRoute { .. }
                    | NavigationHandoffRequest::PopRoute
                    | NavigationHandoffRequest::PopToRoot
            )
        }) {
            NavigationHandoffNextAction::ApplyNativeNavigationCommand
        } else {
            NavigationHandoffNextAction::UseHostedHistoryBridge
        }
    }

    /// Validate the complete handoff.
    pub fn validate(&self) -> Result<(), SharedString> {
        if self.requests.is_empty() {
            return Err("navigation handoff must include at least one request".into());
        }
        if self.requests.len() > 32 {
            return Err("navigation handoff cannot include more than 32 requests".into());
        }
        for request in &self.requests {
            request.validate()?;
        }
        Ok(())
    }

    /// Privacy-preserving summary for logs, tests, and AI-agent traces.
    pub fn to_text(&self) -> String {
        let kinds = self
            .requests
            .iter()
            .map(NavigationHandoffRequest::summary_kind)
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "navigation handoff: requests={} next_action={:?} kinds=[{}]",
            self.request_count(),
            self.next_action(),
            kinds
        )
    }
}

/// Builder for checked navigation handoffs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavigationHandoffBuilder {
    requests: Vec<NavigationHandoffRequest>,
}

impl NavigationHandoffBuilder {
    /// Create an empty navigation handoff builder.
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
        }
    }

    /// Add a native route descriptor.
    pub fn route(mut self, route: NavigationRouteDescriptorBuilder) -> Self {
        if let Ok(route) = route.build_checked() {
            self.requests.push(NavigationHandoffRequest::Route(route));
        }
        self
    }

    /// Add a push-route command.
    pub fn push_route(mut self, route_id: impl Into<SharedString>) -> Self {
        self.requests.push(NavigationHandoffRequest::PushRoute {
            route_id: route_id.into(),
        });
        self
    }

    /// Add a replace-route command.
    pub fn replace_route(mut self, route_id: impl Into<SharedString>) -> Self {
        self.requests.push(NavigationHandoffRequest::ReplaceRoute {
            route_id: route_id.into(),
        });
        self
    }

    /// Add a pop-route command.
    pub fn pop_route(mut self) -> Self {
        self.requests.push(NavigationHandoffRequest::PopRoute);
        self
    }

    /// Add a pop-to-root command.
    pub fn pop_to_root(mut self) -> Self {
        self.requests.push(NavigationHandoffRequest::PopToRoot);
        self
    }

    /// Add a session-restore request with an expected stack depth.
    pub fn restore_session(mut self, stack_depth: usize) -> Self {
        self.requests
            .push(NavigationHandoffRequest::RestoreSession { stack_depth });
        self
    }

    /// Add an incoming deep-link route request.
    pub fn deep_link(mut self, scheme: impl Into<SharedString>) -> Self {
        self.requests.push(NavigationHandoffRequest::DeepLink {
            scheme: scheme.into(),
        });
        self
    }

    /// Add a hosted browser-history bridge request.
    pub fn hosted_history_bridge(mut self, surface_id: impl Into<SharedString>) -> Self {
        self.requests
            .push(NavigationHandoffRequest::HostedHistoryBridge {
                surface_id: surface_id.into(),
            });
        self
    }

    /// Validate the handoff without consuming the builder.
    pub fn validate(&self) -> Result<(), SharedString> {
        self.as_handoff().validate()
    }

    /// Build a checked navigation handoff.
    pub fn build_checked(self) -> Result<NavigationHandoff, SharedString> {
        let handoff = self.as_handoff();
        handoff.validate()?;
        Ok(handoff)
    }

    fn as_handoff(&self) -> NavigationHandoff {
        NavigationHandoff {
            requests: self.requests.clone(),
        }
    }
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
    /// Stable text key for diagnostics and generated tests.
    pub fn to_text(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SlideLeft => "slide_left",
            Self::SlideRight => "slide_right",
            Self::SlideUp => "slide_up",
            Self::SlideDown => "slide_down",
            Self::Fade => "fade",
            Self::Custom(_) => "custom",
        }
    }

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

    /// Byte length of the current route identifier, without exposing it.
    pub fn current_route_id_len_bytes(&self) -> Option<usize> {
        self.current_route().map(Route::id_len_bytes)
    }

    /// Returns true when a route transition is currently active.
    pub fn has_active_transition(&self) -> bool {
        self.transition.is_some()
    }

    /// Stable key for the active transition, or `none`.
    pub fn active_transition_key(&self) -> &'static str {
        self.transition
            .as_ref()
            .map_or("none", |active| active.transition.to_text())
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        let current_route_id_len = self
            .current_route_id_len_bytes()
            .map_or_else(|| "none".to_string(), |len| len.to_string());

        format!(
            "navigator(stack_depth={}, empty={}, has_current_route={}, current_route_id_len_bytes={}, has_active_transition={}, active_transition={})",
            self.len(),
            self.is_empty(),
            self.current_route().is_some(),
            current_route_id_len,
            self.has_active_transition(),
            self.active_transition_key()
        )
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

fn validate_navigation_route_id(route_id: &str) -> Result<(), SharedString> {
    validate_navigation_text(route_id, "navigation route id", 128)?;
    if route_id.contains("://") {
        return Err("navigation route id cannot be a URL".into());
    }
    Ok(())
}

fn validate_navigation_surface_id(surface_id: &str) -> Result<(), SharedString> {
    validate_navigation_text(surface_id, "hosted navigation surface id", 64)?;
    if !surface_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(
            "hosted navigation surface id must contain only ASCII letters, digits, '-', '_', or '.'"
                .into(),
        );
    }
    Ok(())
}

fn validate_navigation_scheme(scheme: &str) -> Result<(), SharedString> {
    validate_navigation_text(scheme, "navigation deep-link scheme", 64)?;
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return Err("navigation deep-link scheme cannot be empty".into());
    };
    if !first.is_ascii_alphabetic() {
        return Err("navigation deep-link scheme must start with an ASCII letter".into());
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')) {
        return Err(
            "navigation deep-link scheme must contain only ASCII letters, digits, '+', '-', or '.'"
                .into(),
        );
    }
    Ok(())
}

fn validate_navigation_text(
    value: &str,
    label: &str,
    max_chars: usize,
) -> Result<(), SharedString> {
    if value.trim().is_empty() {
        return Err(format!("{label} cannot be empty").into());
    }
    if value != value.trim() {
        return Err(format!("{label} cannot have leading or trailing whitespace").into());
    }
    if value.chars().count() > max_chars {
        return Err(format!("{label} cannot be longer than {max_chars} characters").into());
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        NavigationHandoff, NavigationHandoffBuilder, NavigationHandoffNextAction,
        NavigationHandoffRequest, NavigationRouteDescriptorBuilder, Navigator, Route,
        RouteChangeEvent, Transition, navigator,
    };
    use crate::{AppContext, EmptyView, TestAppContext};

    #[test]
    fn navigation_handoff_builder_validates_native_navigation_surface() {
        let handoff = NavigationHandoff::builder()
            .route(
                NavigationRouteDescriptorBuilder::new("home")
                    .restorable_state()
                    .require_activation(),
            )
            .push_route("settings/profile")
            .replace_route("settings/billing")
            .pop_route()
            .pop_to_root()
            .restore_session(3)
            .deep_link("kael")
            .hosted_history_bridge("docs")
            .build_checked()
            .unwrap();

        assert_eq!(handoff.request_count(), 8);
        assert!(handoff.has_native_routes());
        assert!(handoff.has_session_restore());
        assert!(handoff.has_deep_link());
        assert!(handoff.has_hosted_history_bridge());
        assert_eq!(
            handoff.next_action(),
            NavigationHandoffNextAction::ValidateNativeRoutes
        );
        assert!(handoff.to_text().contains("navigation handoff"));
        assert!(handoff.to_text().contains("hosted-history-bridge"));
        assert!(!handoff.to_text().contains("settings/profile"));
        assert!(!handoff.to_text().contains("kael"));

        let route = match &handoff.requests()[0] {
            NavigationHandoffRequest::Route(route) => route,
            request => panic!("unexpected request {request:?}"),
        };
        assert_eq!(route.id(), "home");
        assert!(route.has_restorable_state());
        assert!(route.requires_activation());
        assert!(route.to_text().contains("id_len_bytes=4"));
        assert!(!route.to_text().contains("home"));
    }

    #[test]
    fn navigation_handoff_builder_rejects_unsafe_shapes() {
        assert!(
            NavigationRouteDescriptorBuilder::new("")
                .validate()
                .is_err()
        );
        assert!(
            NavigationRouteDescriptorBuilder::new(" https://example.com")
                .validate()
                .is_err()
        );
        assert!(
            NavigationRouteDescriptorBuilder::new("https://example.com")
                .validate()
                .is_err()
        );
        assert!(NavigationHandoffBuilder::new().validate().is_err());
        assert!(
            NavigationHandoffBuilder::new()
                .restore_session(0)
                .validate()
                .is_err()
        );
        assert!(
            NavigationHandoffBuilder::new()
                .deep_link("1bad")
                .validate()
                .is_err()
        );
        assert!(
            NavigationHandoffBuilder::new()
                .hosted_history_bridge("bad surface")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn navigation_handoff_next_action_prioritizes_restore_and_hosted_fallback() {
        let restore = NavigationHandoffBuilder::new()
            .restore_session(2)
            .push_route("thread")
            .build_checked()
            .unwrap();
        assert_eq!(
            restore.next_action(),
            NavigationHandoffNextAction::RestoreSessionStack
        );

        let command = NavigationHandoffBuilder::new()
            .replace_route("settings")
            .build_checked()
            .unwrap();
        assert_eq!(
            command.next_action(),
            NavigationHandoffNextAction::ApplyNativeNavigationCommand
        );

        let hosted = NavigationHandoffBuilder::new()
            .hosted_history_bridge("checkout")
            .build_checked()
            .unwrap();
        assert_eq!(
            hosted.next_action(),
            NavigationHandoffNextAction::UseHostedHistoryBridge
        );
    }

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

    #[kael::test]
    fn navigator_summary_is_content_safe(cx: &mut TestAppContext) {
        let route = Route::new("private/home", cx.new(|_| EmptyView)).with_memento("secret");
        assert_eq!(route.id_len_bytes(), "private/home".len());
        assert!(route.has_memento());
        let route_summary = route.to_text();
        assert!(route_summary.contains("has_memento=true"));
        assert!(!route_summary.contains("private/home"));
        assert!(!route_summary.contains("secret"));

        let mut navigator = Navigator::new(route);
        assert_eq!(
            navigator.current_route_id_len_bytes(),
            Some("private/home".len())
        );
        assert!(!navigator.has_active_transition());
        assert_eq!(navigator.active_transition_key(), "none");

        navigator.replace_stack_routes(
            vec![
                Route::new("private/home", cx.new(|_| EmptyView)),
                Route::new("billing/secret", cx.new(|_| EmptyView)),
            ],
            Transition::Fade,
        );

        assert_eq!(navigator.len(), 2);
        assert!(navigator.has_active_transition());
        assert_eq!(navigator.active_transition_key(), "fade");

        let navigator_summary = navigator.to_text();
        assert!(navigator_summary.contains("navigator(stack_depth=2"));
        assert!(navigator_summary.contains("active_transition=fade"));
        assert!(!navigator_summary.contains("private/home"));
        assert!(!navigator_summary.contains("billing/secret"));

        let event = RouteChangeEvent {
            previous_route_id: Some("private/home".into()),
            current_route_id: Some("billing/secret".into()),
            stack_depth: 2,
        };
        assert!(event.has_previous_route());
        assert!(event.has_current_route());
        let event_summary = event.to_text();
        assert!(event_summary.contains("stack_depth=2"));
        assert!(!event_summary.contains("private/home"));
        assert!(!event_summary.contains("billing/secret"));
    }
}

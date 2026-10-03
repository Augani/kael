//! Connect controller notifications to the containing view while mounted.
use kael::*;
struct ModelObservation {
    entity_id: EntityId,
    _subscription: Subscription,
}

/// Keyed ownership releases the observation on unmount. Context::observe uses a
/// weak handle, so this cannot keep either the controller or view alive.
pub(crate) fn observe_model<T: 'static>(
    id: ElementId,
    entity: &Entity<T>,
    window: &mut Window,
    cx: &mut App,
) {
    let observer = window.use_keyed_state(
        ElementId::NamedChild(Box::new(id), "model-observer".into()),
        cx,
        |_, cx| ModelObservation {
            entity_id: entity.entity_id(),
            _subscription: cx.observe(entity, |_, _, cx| cx.notify()),
        },
    );
    observer.update(cx, |observer, cx| {
        if observer.entity_id != entity.entity_id() {
            observer.entity_id = entity.entity_id();
            observer._subscription = cx.observe(entity, |_, _, cx| cx.notify());
        }
    });
}

#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

extern crate self as kael_refineable;

pub use derive_refineable::Refineable;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CASCADE_ID: AtomicU64 = AtomicU64::new(1);

/// A trait for types that can be refined with partial updates.
///
/// The `Refineable` trait enables hierarchical configuration patterns where a base configuration
/// can be selectively overridden by refinements. This is particularly useful for styling and
/// settings, and theme hierarchies.
///
/// # Derive Macro
///
/// The `#[derive(Refineable)]` macro automatically generates a companion refinement type and
/// implements this trait. For a struct `Style`, it creates `StyleRefinement` where each field is
/// wrapped appropriately:
///
/// - **Refineable fields** (marked with `#[refineable]`): Become the corresponding refinement type
///   (e.g., `Bar` becomes `BarRefinement`)
/// - **Optional fields** (`Option<T>`): Remain as `Option<T>`
/// - **Regular fields**: Become `Option<T>`
///
/// ## Attributes
///
/// The derive macro supports these attributes on the struct:
/// - `#[refineable(Debug)]`: Implements `Debug` for the refinement type
/// - `#[refineable(Serialize)]`: Derives `Serialize` which skips serializing `None`
/// - `#[refineable(Deserialize)]`: Derives `Deserialize` and defaults missing nested refinements
/// - `#[refineable(OtherTrait)]`: Derives additional traits on the refinement type
///
/// Fields can be marked with:
/// - `#[refineable]`: Field is itself refineable (uses nested refinement type)
pub trait Refineable: Clone {
    /// The partial-update type accepted by this value.
    type Refinement: Refineable<Refinement = Self::Refinement> + IsEmpty + Default;

    /// Applies the given refinement to this instance, modifying it in place.
    ///
    /// Only non-empty values in the refinement are applied.
    ///
    /// * For refineable fields, this recursively calls `refine`.
    /// * For other fields, the value is replaced if present in the refinement.
    fn refine(&mut self, refinement: &Self::Refinement);

    /// Returns a new instance with the refinement applied, equivalent to cloning `self` and calling
    /// `refine` on it.
    #[must_use]
    fn refined(self, refinement: Self::Refinement) -> Self;

    /// Creates an instance from a cascade by merging all refinements atop the default value.
    #[must_use]
    fn from_cascade(cascade: &Cascade<Self>) -> Self
    where
        Self: Default + Sized,
    {
        Self::default().refined(cascade.merged())
    }

    /// Returns `true` if this instance would contain all values from the refinement.
    ///
    /// For refineable fields, this recursively checks `is_superset_of`. For other fields, this
    /// checks if the refinement's `Some` values match this instance's values.
    #[must_use]
    fn is_superset_of(&self, refinement: &Self::Refinement) -> bool;

    /// Returns a refinement that represents the difference between this instance and the given
    /// refinement.
    ///
    /// For refineable fields, this recursively calls `subtract`. For other fields, the field is
    /// `None` if the field's value is equal to the refinement.
    #[must_use]
    fn subtract(&self, refinement: &Self::Refinement) -> Self::Refinement;
}

/// Reports whether a refinement contains any effective changes.
pub trait IsEmpty {
    /// Returns `true` if applying this refinement would have no effect.
    #[must_use]
    fn is_empty(&self) -> bool;
}

/// A cascade of refinements that can be merged in priority order.
///
/// A cascade maintains a sequence of optional refinements where later entries
/// take precedence over earlier ones. The first slot (index 0) is always the
/// base refinement and is guaranteed to be present.
///
/// This is useful for implementing configuration hierarchies like CSS cascading,
/// where styles from different sources (user agent, user, author) are combined
/// with specific precedence rules.
pub struct Cascade<S: Refineable> {
    id: u64,
    refinements: Vec<Option<S::Refinement>>,
}

impl<S: Refineable> Default for Cascade<S> {
    fn default() -> Self {
        Self {
            id: NEXT_CASCADE_ID.fetch_add(1, Ordering::Relaxed),
            refinements: vec![Some(Default::default())],
        }
    }
}

/// A handle to a specific slot in a cascade.
///
/// Slots are used to identify specific positions in the cascade where
/// refinements can be set or updated.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CascadeSlot {
    cascade_id: u64,
    index: usize,
}

/// Error returned when a slot does not belong to the target cascade.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidCascadeSlot;

impl std::fmt::Display for InvalidCascadeSlot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("cascade slot does not belong to this cascade")
    }
}

impl std::error::Error for InvalidCascadeSlot {}

impl<S: Refineable> Cascade<S> {
    /// Reserves a new slot in the cascade and returns a handle to it.
    ///
    /// The new slot is initially empty (`None`) and can be populated later
    /// using `set()`.
    #[must_use]
    pub fn reserve(&mut self) -> CascadeSlot {
        self.refinements.push(None);
        CascadeSlot {
            cascade_id: self.id,
            index: self.refinements.len() - 1,
        }
    }

    /// Returns a mutable reference to the base refinement (slot 0).
    ///
    /// The base refinement is always present and serves as the foundation
    /// for the cascade.
    pub fn base(&mut self) -> &mut S::Refinement {
        self.refinements[0]
            .as_mut()
            .expect("cascade base refinement is an internal invariant")
    }

    /// Sets the refinement for a specific slot in the cascade.
    ///
    /// Setting a slot to `None` effectively removes it from consideration
    /// during merging.
    pub fn set(
        &mut self,
        slot: CascadeSlot,
        refinement: Option<S::Refinement>,
    ) -> Result<(), InvalidCascadeSlot> {
        if slot.cascade_id != self.id {
            return Err(InvalidCascadeSlot);
        }
        let Some(target) = self.refinements.get_mut(slot.index) else {
            return Err(InvalidCascadeSlot);
        };
        *target = refinement;
        Ok(())
    }

    /// Merges all refinements in the cascade into a single refinement.
    ///
    /// Refinements are applied in order, with later slots taking precedence.
    /// Empty slots (`None`) are skipped during merging.
    #[must_use]
    pub fn merged(&self) -> S::Refinement {
        let mut merged = self.refinements[0]
            .clone()
            .expect("cascade base refinement is an internal invariant");
        for refinement in self.refinements.iter().skip(1).flatten() {
            merged.refine(refinement);
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::{Cascade, IsEmpty, Refineable};

    #[derive(Clone, Default, Refineable)]
    #[refineable(Debug)]
    struct EmptyStyle {}

    mod theme {
        use super::Refineable;

        #[derive(Clone, Default, Refineable)]
        pub struct Child<T: Clone + Default> {
            pub value: T,
        }
    }

    #[derive(Clone, Default, Refineable)]
    struct ParentStyle {
        #[refineable]
        child: theme::Child<u8>,
    }

    #[derive(Clone, Default, Refineable)]
    struct QualifiedOptionalStyle {
        value: std::option::Option<u8>,
    }

    #[derive(Clone, Default, Refineable)]
    #[refineable(Debug)]
    struct DebugStyle<T> {
        value: T,
    }

    #[derive(Clone, Default, Refineable)]
    struct GenericContainerStyle<T> {
        values: Vec<T>,
    }

    #[derive(Clone, Default, Refineable)]
    #[refineable(serde::Serialize, serde::Deserialize)]
    struct SerializableChild {
        value: u8,
    }

    #[derive(Clone, Default, Refineable)]
    #[refineable(serde::Serialize, serde::Deserialize)]
    struct SerializableParent {
        #[refineable]
        child: SerializableChild,
        enabled: bool,
    }

    #[derive(Clone, Default, Refineable)]
    #[refineable(serde::Deserialize)]
    struct DeserializeOnlyChild {
        value: u8,
    }

    #[derive(Clone, Default, Refineable)]
    #[refineable(serde::Deserialize)]
    struct DeserializeOnlyParent {
        #[refineable]
        child: DeserializeOnlyChild,
    }

    mod qualified_derive_without_trait_import {
        #[derive(Clone, Default, kael_refineable::Refineable)]
        pub struct Values {
            pub value: u8,
        }

        #[derive(Clone, Default, kael_refineable::Refineable)]
        pub struct Settings {
            #[refineable]
            values: Values,
        }

        pub fn refined_value() -> u8 {
            let mut settings = Settings::default();
            kael_refineable::Refineable::refine(
                &mut settings,
                &SettingsRefinement {
                    values: ValuesRefinement { value: Some(7) },
                },
            );
            settings.values.value
        }
    }

    #[derive(Clone, Default, PartialEq, Eq)]
    struct TestStyle {
        value: i32,
    }

    #[derive(Clone, Default, PartialEq, Eq)]
    struct TestRefinement {
        value: Option<i32>,
    }

    impl IsEmpty for TestRefinement {
        fn is_empty(&self) -> bool {
            self.value.is_none()
        }
    }

    impl Refineable for TestStyle {
        type Refinement = TestRefinement;

        fn refine(&mut self, refinement: &Self::Refinement) {
            if let Some(value) = refinement.value {
                self.value = value;
            }
        }

        fn refined(mut self, refinement: Self::Refinement) -> Self {
            self.refine(&refinement);
            self
        }

        fn is_superset_of(&self, refinement: &Self::Refinement) -> bool {
            refinement.value.is_none_or(|value| self.value == value)
        }

        fn subtract(&self, refinement: &Self::Refinement) -> Self::Refinement {
            TestRefinement {
                value: (refinement.value != Some(self.value)).then_some(self.value),
            }
        }
    }

    impl Refineable for TestRefinement {
        type Refinement = Self;

        fn refine(&mut self, refinement: &Self::Refinement) {
            if refinement.value.is_some() {
                self.value = refinement.value;
            }
        }

        fn refined(mut self, refinement: Self::Refinement) -> Self {
            self.refine(&refinement);
            self
        }

        fn is_superset_of(&self, refinement: &Self::Refinement) -> bool {
            refinement.value.is_none() || self.value == refinement.value
        }

        fn subtract(&self, refinement: &Self::Refinement) -> Self::Refinement {
            TestRefinement {
                value: if self.value != refinement.value {
                    self.value
                } else {
                    None
                },
            }
        }
    }

    #[test]
    fn slots_are_bound_to_their_own_cascade() {
        let mut first = Cascade::<TestStyle>::default();
        let slot = first.reserve();
        first
            .set(slot, Some(TestRefinement { value: Some(7) }))
            .unwrap();
        assert_eq!(first.merged().value, Some(7));

        let mut second = Cascade::<TestStyle>::default();
        let _second_slot = second.reserve();
        assert!(
            second
                .set(slot, Some(TestRefinement { value: Some(9) }))
                .is_err()
        );
        assert!(second.merged().is_empty());
    }

    #[test]
    fn derived_empty_struct_has_an_empty_refinement() {
        let _style = EmptyStyle {};
        let refinement = EmptyStyleRefinement::default();

        assert!(refinement.is_empty());
        assert!(!refinement.is_some());
        assert_eq!(format!("{refinement:?}"), "EmptyStyleRefinement");
    }

    #[test]
    fn derived_nested_refinement_keeps_its_module_path() {
        let refinement = ParentStyleRefinement {
            child: theme::ChildRefinement { value: Some(7) },
        };
        let style = ParentStyle::default().refined(refinement);

        assert_eq!(style.child.value, 7);
    }

    #[test]
    fn qualified_option_fields_keep_optional_refinement_semantics() {
        let style = QualifiedOptionalStyle::default()
            .refined(QualifiedOptionalStyleRefinement { value: Some(7) });

        assert_eq!(style.value, Some(7));
    }

    #[test]
    fn debug_derivation_supports_generic_fields() {
        let _style = DebugStyle { value: 0_u8 };
        let refinement = DebugStyleRefinement::<u8> { value: Some(7) };

        assert_eq!(
            format!("{refinement:?}"),
            "DebugStyleRefinement { value: Some(7) }"
        );
    }

    #[test]
    fn generic_container_fields_use_their_actual_trait_bounds() {
        let style =
            GenericContainerStyle::<u8>::default().refined(GenericContainerStyleRefinement {
                values: Some(vec![1, 2, 3]),
            });

        assert_eq!(style.values, [1, 2, 3]);
    }

    #[test]
    fn serde_derivation_omits_and_defaults_empty_fields() {
        let _parent = SerializableParent::default();
        let empty = SerializableParentRefinement::default();
        assert_eq!(serde_json::to_string(&empty).unwrap(), "{}");

        let refinement: SerializableParentRefinement =
            serde_json::from_str(r#"{"child":{"value":7}}"#).unwrap();
        assert_eq!(refinement.child.value, Some(7));
        assert_eq!(refinement.enabled, None);
    }

    #[test]
    fn deserialize_only_nested_refinements_default_missing_fields() {
        let refinement: DeserializeOnlyParentRefinement = serde_json::from_str("{}").unwrap();

        assert!(refinement.is_empty());
        assert_eq!(
            DeserializeOnlyParent::default()
                .refined(refinement)
                .child
                .value,
            0
        );
    }

    #[test]
    fn qualified_derive_does_not_require_a_trait_import() {
        assert_eq!(qualified_derive_without_trait_import::refined_value(), 7);
    }
}

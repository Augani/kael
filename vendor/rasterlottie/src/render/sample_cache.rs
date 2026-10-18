use std::{cell::RefCell, ptr};

use rustc_hash::FxHashMap;

use crate::{
    AnimatedValue, BezierPath, ShapePathValue,
    timeline::{
        NumericKeyframe, ShapeKeyframe, parse_numeric_keyframes, parse_shape_keyframes,
        sample_numbers_from_keyframes, sample_scalar_from_keyframes,
        sample_shape_path_from_keyframes, sample_static_numbers, sample_vec2_from_keyframes,
    },
};

#[derive(Debug, Default)]
pub(super) struct TimelineSampleCache {
    numeric: RefCell<FxHashMap<usize, Option<Vec<NumericKeyframe>>>>,
    shapes: RefCell<FxHashMap<usize, Option<Vec<ShapeKeyframe>>>>,
}

impl TimelineSampleCache {
    pub(super) fn sample_numbers(&self, value: &AnimatedValue, frame: f32) -> Option<Vec<f32>> {
        if value.is_static() {
            return sample_static_numbers(value);
        }

        let key = ptr::from_ref(value) as usize;
        if let Some(keyframes) = self.numeric.borrow().get(&key).cloned() {
            return keyframes
                .as_deref()
                .and_then(|keyframes| sample_numbers_from_keyframes(keyframes, frame));
        }

        let parsed = parse_numeric_keyframes(value);
        let sampled = parsed
            .as_deref()
            .and_then(|keyframes| sample_numbers_from_keyframes(keyframes, frame));
        self.numeric.borrow_mut().insert(key, parsed);
        sampled
    }

    pub(super) fn sample_scalar(&self, value: &AnimatedValue, frame: f32) -> Option<f32> {
        if value.is_static() {
            return value.as_scalar();
        }

        let key = ptr::from_ref(value) as usize;
        if let Some(keyframes) = self.numeric.borrow().get(&key).cloned() {
            return keyframes
                .as_deref()
                .and_then(|keyframes| sample_scalar_from_keyframes(keyframes, frame));
        }

        let parsed = parse_numeric_keyframes(value);
        let sampled = parsed
            .as_deref()
            .and_then(|keyframes| sample_scalar_from_keyframes(keyframes, frame));
        self.numeric.borrow_mut().insert(key, parsed);
        sampled
    }

    pub(super) fn sample_vec2(&self, value: &AnimatedValue, frame: f32) -> Option<[f32; 2]> {
        if value.is_static() {
            return value.as_vec2();
        }

        let key = ptr::from_ref(value) as usize;
        if let Some(keyframes) = self.numeric.borrow().get(&key).cloned() {
            return keyframes
                .as_deref()
                .and_then(|keyframes| sample_vec2_from_keyframes(keyframes, frame));
        }

        let parsed = parse_numeric_keyframes(value);
        let sampled = parsed
            .as_deref()
            .and_then(|keyframes| sample_vec2_from_keyframes(keyframes, frame));
        self.numeric.borrow_mut().insert(key, parsed);
        sampled
    }

    pub(super) fn sample_shape_path(
        &self,
        value: &ShapePathValue,
        frame: f32,
    ) -> Option<BezierPath> {
        if value.is_static() {
            return value.as_bezier_path();
        }

        let key = ptr::from_ref(value) as usize;
        if let Some(keyframes) = self.shapes.borrow().get(&key).cloned() {
            return keyframes
                .as_deref()
                .and_then(|keyframes| sample_shape_path_from_keyframes(keyframes, frame));
        }

        let parsed = parse_shape_keyframes(value);
        let sampled = parsed
            .as_deref()
            .and_then(|keyframes| sample_shape_path_from_keyframes(keyframes, frame));
        self.shapes.borrow_mut().insert(key, parsed);
        sampled
    }
}

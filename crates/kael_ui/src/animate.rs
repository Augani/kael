use kael::*;
use std::time::Duration;

use crate::animations::easings;

#[derive(Clone)]
pub struct AnimationPreset {
    duration: Duration,
    easing: fn(f32) -> f32,
    delay: Duration,
}

impl AnimationPreset {
    pub fn new(duration: Duration, easing: fn(f32) -> f32) -> Self {
        Self {
            duration,
            easing,
            delay: Duration::ZERO,
        }
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn easing(mut self, easing: fn(f32) -> f32) -> Self {
        self.easing = easing;
        self
    }

    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn with_easing(mut self, easing: fn(f32) -> f32) -> Self {
        self.easing = easing;
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn get_duration(&self) -> Duration {
        self.duration
    }

    pub fn get_easing(&self) -> fn(f32) -> f32 {
        self.easing
    }

    pub fn get_delay(&self) -> Duration {
        self.delay
    }

    pub fn to_animation(&self) -> Animation {
        Animation::new(self.duration).with_easing(self.easing)
    }
}

pub fn fade_in() -> AnimationPreset {
    AnimationPreset::new(Duration::from_millis(200), easings::ease_out_cubic)
}

pub fn fade_out() -> AnimationPreset {
    AnimationPreset::new(Duration::from_millis(200), easings::ease_in_cubic)
}

pub fn slide_up() -> AnimationPreset {
    AnimationPreset::new(Duration::from_millis(250), easings::ease_out_cubic)
}

pub fn slide_down() -> AnimationPreset {
    AnimationPreset::new(Duration::from_millis(250), easings::ease_out_cubic)
}

pub fn scale_in() -> AnimationPreset {
    AnimationPreset::new(Duration::from_millis(200), easings::ease_out_back)
}

pub fn bounce_in() -> AnimationPreset {
    AnimationPreset::new(Duration::from_millis(400), easings::elastic)
}

pub fn slide_in_left() -> AnimationPreset {
    AnimationPreset::new(Duration::from_millis(250), easings::ease_out_cubic)
}

pub fn slide_in_right() -> AnimationPreset {
    AnimationPreset::new(Duration::from_millis(250), easings::ease_out_cubic)
}

#[derive(Clone)]
pub struct StaggerConfig {
    delay_per_child: Duration,
    preset: AnimationPreset,
}

impl StaggerConfig {
    pub fn new() -> Self {
        Self {
            delay_per_child: Duration::from_millis(50),
            preset: fade_in(),
        }
    }

    pub fn delay_per_child(mut self, delay: Duration) -> Self {
        self.delay_per_child = delay;
        self
    }

    pub fn animation(mut self, preset: AnimationPreset) -> Self {
        self.preset = preset;
        self
    }

    pub fn delay_for_index(&self, index: usize) -> Duration {
        self.preset.delay + self.delay_per_child * index as u32
    }

    pub fn preset_for_index(&self, index: usize) -> AnimationPreset {
        let mut preset = self.preset.clone();
        preset.delay = self.delay_for_index(index);
        preset
    }

    pub fn get_preset(&self) -> &AnimationPreset {
        &self.preset
    }
}

impl Default for StaggerConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct Transition {
    property: SharedString,
    duration: Duration,
    easing: fn(f32) -> f32,
    delay: Duration,
}

impl Transition {
    pub fn new(property: impl Into<SharedString>) -> Self {
        Self {
            property: property.into(),
            duration: Duration::from_millis(200),
            easing: easings::ease_out_cubic,
            delay: Duration::ZERO,
        }
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn easing(mut self, easing: fn(f32) -> f32) -> Self {
        self.easing = easing;
        self
    }

    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn get_property(&self) -> &SharedString {
        &self.property
    }

    pub fn get_duration(&self) -> Duration {
        self.duration
    }

    pub fn get_easing(&self) -> fn(f32) -> f32 {
        self.easing
    }

    pub fn get_delay(&self) -> Duration {
        self.delay
    }

    pub fn to_animation(&self) -> Animation {
        Animation::new(self.duration).with_easing(self.easing)
    }
}

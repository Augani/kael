//! Serializable per-clip effects.
//!
//! [`ClipEffect`] is a small, project-storable description of one reference-compositor
//! effect pass, and knows both the op it maps to and the cache parameter hash it
//! contributes. [`EffectStack`] is an ordered list applied to a clip's source image before
//! compositing — the bridge between the reference effect library and the timeline/project
//! model (V5 keyframe params, P2-B effect stacks, P4-C project format).

use kael_render_graph::reference::{
    Image, PassOp, exposure, gamma, gaussian_blur, saturation, solarize, temperature_tint, vignette,
};
use serde::{Deserialize, Serialize};

use crate::automation::Automation;

/// A single serializable image effect applied to a clip before compositing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ClipEffect {
    /// Gaussian blur of standard deviation `sigma` pixels.
    GaussianBlur {
        /// Blur radius (standard deviation) in pixels.
        sigma: f32,
    },
    /// Exposure adjustment in stops (`+1` doubles each channel).
    Exposure {
        /// Stops of exposure.
        stops: f32,
    },
    /// Gamma curve `value^power`.
    Gamma {
        /// Gamma power.
        power: f32,
    },
    /// Saturation scale (`1` unchanged, `0` grayscale).
    Saturation {
        /// Saturation multiplier.
        amount: f32,
    },
    /// White-balance temperature/tint, each normalized to `-1..=1`.
    WhiteBalance {
        /// Warm/cool axis.
        temperature: f32,
        /// Green/magenta axis.
        tint: f32,
    },
    /// Radial vignette darkening toward the edges.
    Vignette {
        /// Darkening strength.
        amount: f32,
        /// Normalized radius where darkening begins.
        radius: f32,
        /// Falloff width.
        softness: f32,
    },
    /// Solarize (Sabattier) fold at `threshold`.
    Solarize {
        /// Fold threshold.
        threshold: f32,
    },
}

impl ClipEffect {
    /// The reference compositor op that applies this effect to a single input image.
    pub fn to_pass_op(self) -> PassOp<'static> {
        match self.sanitized() {
            ClipEffect::GaussianBlur { sigma } => gaussian_blur(sigma),
            ClipEffect::Exposure { stops } => exposure(stops),
            ClipEffect::Gamma { power } => gamma(power),
            ClipEffect::Saturation { amount } => saturation(amount),
            ClipEffect::WhiteBalance { temperature, tint } => temperature_tint(temperature, tint),
            ClipEffect::Vignette {
                amount,
                radius,
                softness,
            } => vignette(amount, radius, softness),
            ClipEffect::Solarize { threshold } => solarize(threshold),
        }
    }

    /// A stable hash of this effect's identity and parameters, for render-graph cache keys.
    pub fn param_hash(self) -> u64 {
        let (tag, params): (u8, [f32; 3]) = match self.sanitized() {
            ClipEffect::GaussianBlur { sigma } => (1, [sigma, 0.0, 0.0]),
            ClipEffect::Exposure { stops } => (2, [stops, 0.0, 0.0]),
            ClipEffect::Gamma { power } => (3, [power, 0.0, 0.0]),
            ClipEffect::Saturation { amount } => (4, [amount, 0.0, 0.0]),
            ClipEffect::WhiteBalance { temperature, tint } => (5, [temperature, tint, 0.0]),
            ClipEffect::Vignette {
                amount,
                radius,
                softness,
            } => (6, [amount, radius, softness]),
            ClipEffect::Solarize { threshold } => (7, [threshold, 0.0, 0.0]),
        };
        let mut hash = fnv_mix(FNV_OFFSET, tag as u64);
        for value in params {
            hash = fnv_mix(hash, value.to_bits() as u64);
        }
        hash
    }

    fn sanitized(self) -> Self {
        match self {
            Self::GaussianBlur { sigma } => Self::GaussianBlur {
                sigma: finite_or(sigma, 0.0),
            },
            Self::Exposure { stops } => Self::Exposure {
                stops: finite_or(stops, 0.0),
            },
            Self::Gamma { power } => Self::Gamma {
                power: finite_or(power, 1.0),
            },
            Self::Saturation { amount } => Self::Saturation {
                amount: finite_or(amount, 1.0),
            },
            Self::WhiteBalance { temperature, tint } => Self::WhiteBalance {
                temperature: finite_or(temperature, 0.0),
                tint: finite_or(tint, 0.0),
            },
            Self::Vignette {
                amount,
                radius,
                softness,
            } => Self::Vignette {
                amount: finite_or(amount, 0.0),
                radius: finite_or(radius, 0.5),
                softness: finite_or(softness, 0.5),
            },
            Self::Solarize { threshold } => Self::Solarize {
                threshold: finite_or(threshold, 2.0),
            },
        }
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        // Canonicalize signed zero so equivalent renders share one cache key.
        if value == 0.0 { 0.0 } else { value }
    } else {
        fallback
    }
}

/// An ordered stack of [`ClipEffect`]s applied to a clip's source image.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectStack {
    /// The effects, applied in order (the first listed is applied first).
    pub effects: Vec<ClipEffect>,
}

impl EffectStack {
    /// An empty stack (the identity).
    pub fn new() -> Self {
        Self {
            effects: Vec::new(),
        }
    }

    /// Whether the stack has no effects.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// The number of effects in the stack.
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Apply every effect in order to `source`, returning the result. An empty stack
    /// returns an unchanged copy.
    pub fn apply(&self, source: &Image) -> Image {
        let mut current = source.clone();
        for effect in &self.effects {
            let mut next = Image::new(current.width, current.height);
            effect.to_pass_op()(&[&current], &mut next);
            current = next;
        }
        current
    }

    /// A stable hash folding every effect's parameter hash in order, for cache keys. The
    /// order matters: `[a, b]` and `[b, a]` hash differently.
    pub fn param_hash(&self) -> u64 {
        let mut hash = FNV_OFFSET;
        for effect in &self.effects {
            hash = fnv_mix(hash, effect.param_hash());
        }
        hash
    }
}

/// A [`ClipEffect`] whose scalar parameters are [`Automation`] curves rather than
/// constants — a keyframed effect (P2-C), built on the existing automation core.
/// [`resolve`](AnimatedEffect::resolve) samples every curve at a time to produce the
/// concrete [`ClipEffect`] for that moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnimatedEffect {
    /// Keyframed Gaussian blur radius.
    GaussianBlur {
        /// Blur standard deviation over time.
        sigma: Automation,
    },
    /// Keyframed exposure.
    Exposure {
        /// Stops over time.
        stops: Automation,
    },
    /// Keyframed gamma.
    Gamma {
        /// Gamma power over time.
        power: Automation,
    },
    /// Keyframed saturation.
    Saturation {
        /// Saturation multiplier over time.
        amount: Automation,
    },
    /// Keyframed white balance.
    WhiteBalance {
        /// Warm/cool axis over time.
        temperature: Automation,
        /// Green/magenta axis over time.
        tint: Automation,
    },
    /// Keyframed vignette.
    Vignette {
        /// Darkening strength over time.
        amount: Automation,
        /// Radius over time.
        radius: Automation,
        /// Falloff width over time.
        softness: Automation,
    },
    /// Keyframed solarize threshold.
    Solarize {
        /// Fold threshold over time.
        threshold: Automation,
    },
}

impl AnimatedEffect {
    /// Sample every parameter curve at `time_ms` to produce the concrete effect.
    pub fn resolve(&self, time_ms: u64) -> ClipEffect {
        match self {
            AnimatedEffect::GaussianBlur { sigma } => ClipEffect::GaussianBlur {
                sigma: sigma.sample(time_ms),
            },
            AnimatedEffect::Exposure { stops } => ClipEffect::Exposure {
                stops: stops.sample(time_ms),
            },
            AnimatedEffect::Gamma { power } => ClipEffect::Gamma {
                power: power.sample(time_ms),
            },
            AnimatedEffect::Saturation { amount } => ClipEffect::Saturation {
                amount: amount.sample(time_ms),
            },
            AnimatedEffect::WhiteBalance { temperature, tint } => ClipEffect::WhiteBalance {
                temperature: temperature.sample(time_ms),
                tint: tint.sample(time_ms),
            },
            AnimatedEffect::Vignette {
                amount,
                radius,
                softness,
            } => ClipEffect::Vignette {
                amount: amount.sample(time_ms),
                radius: radius.sample(time_ms),
                softness: softness.sample(time_ms),
            },
            AnimatedEffect::Solarize { threshold } => ClipEffect::Solarize {
                threshold: threshold.sample(time_ms),
            },
        }
    }
}

/// An ordered stack of [`AnimatedEffect`]s — the keyframed counterpart of [`EffectStack`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnimatedEffectStack {
    /// The animated effects, applied in order once resolved.
    pub effects: Vec<AnimatedEffect>,
}

impl AnimatedEffectStack {
    /// An empty stack.
    pub fn new() -> Self {
        Self {
            effects: Vec::new(),
        }
    }

    /// Whether the stack has no effects.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// The number of effects.
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Resolve every animated effect at `time_ms` into a concrete [`EffectStack`].
    pub fn resolve(&self, time_ms: u64) -> EffectStack {
        EffectStack {
            effects: self
                .effects
                .iter()
                .map(|effect| effect.resolve(time_ms))
                .collect(),
        }
    }
}

impl From<ClipEffect> for AnimatedEffect {
    /// Lift a static effect into a keyframed one whose parameters are constant curves.
    fn from(effect: ClipEffect) -> Self {
        match effect {
            ClipEffect::GaussianBlur { sigma } => AnimatedEffect::GaussianBlur {
                sigma: Automation::constant(sigma),
            },
            ClipEffect::Exposure { stops } => AnimatedEffect::Exposure {
                stops: Automation::constant(stops),
            },
            ClipEffect::Gamma { power } => AnimatedEffect::Gamma {
                power: Automation::constant(power),
            },
            ClipEffect::Saturation { amount } => AnimatedEffect::Saturation {
                amount: Automation::constant(amount),
            },
            ClipEffect::WhiteBalance { temperature, tint } => AnimatedEffect::WhiteBalance {
                temperature: Automation::constant(temperature),
                tint: Automation::constant(tint),
            },
            ClipEffect::Vignette {
                amount,
                radius,
                softness,
            } => AnimatedEffect::Vignette {
                amount: Automation::constant(amount),
                radius: Automation::constant(radius),
                softness: Automation::constant(softness),
            },
            ClipEffect::Solarize { threshold } => AnimatedEffect::Solarize {
                threshold: Automation::constant(threshold),
            },
        }
    }
}

impl From<EffectStack> for AnimatedEffectStack {
    /// Lift a static stack into a keyframed one with constant-curve parameters.
    fn from(stack: EffectStack) -> Self {
        AnimatedEffectStack {
            effects: stack
                .effects
                .into_iter()
                .map(AnimatedEffect::from)
                .collect(),
        }
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(mut hash: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::{Interpolation, Keyframe};
    use kael_render_graph::reference::Image;

    #[test]
    fn clip_effect_serde_round_trips() {
        let effects = [
            ClipEffect::GaussianBlur { sigma: 2.5 },
            ClipEffect::Exposure { stops: -1.0 },
            ClipEffect::Gamma { power: 2.2 },
            ClipEffect::Saturation { amount: 0.0 },
            ClipEffect::WhiteBalance {
                temperature: 0.3,
                tint: -0.2,
            },
            ClipEffect::Vignette {
                amount: 0.8,
                radius: 0.2,
                softness: 0.5,
            },
            ClipEffect::Solarize { threshold: 0.5 },
        ];
        for effect in effects {
            let json = serde_json::to_string(&effect).unwrap();
            let restored: ClipEffect = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, effect);
        }
    }

    #[test]
    fn empty_stack_is_identity() {
        let source = Image::filled(4, 4, [0.3, 0.6, 0.9, 1.0]);
        let stack = EffectStack::new();
        assert!(stack.is_empty());
        assert_eq!(stack.apply(&source).pixels, source.pixels);
    }

    #[test]
    fn stack_applies_effects_in_order() {
        let source = Image::filled(2, 2, [0.25, 0.25, 0.25, 1.0]);

        // Exposure +1 doubles (0.25 -> 0.5), then gamma 2 squares (0.5 -> 0.25).
        let forward = EffectStack {
            effects: vec![
                ClipEffect::Exposure { stops: 1.0 },
                ClipEffect::Gamma { power: 2.0 },
            ],
        };
        assert!((forward.apply(&source).pixel(0, 0)[0] - 0.25).abs() < 1e-6);

        // Reversed: gamma 2 first (0.25 -> 0.0625), then exposure +1 (-> 0.125).
        let reversed = EffectStack {
            effects: vec![
                ClipEffect::Gamma { power: 2.0 },
                ClipEffect::Exposure { stops: 1.0 },
            ],
        };
        assert!((reversed.apply(&source).pixel(0, 0)[0] - 0.125).abs() < 1e-6);
    }

    #[test]
    fn param_hash_tracks_params_and_order() {
        // Same params hash equal; different params differ.
        assert_eq!(
            ClipEffect::GaussianBlur { sigma: 1.0 }.param_hash(),
            ClipEffect::GaussianBlur { sigma: 1.0 }.param_hash()
        );
        assert_ne!(
            ClipEffect::GaussianBlur { sigma: 1.0 }.param_hash(),
            ClipEffect::GaussianBlur { sigma: 2.0 }.param_hash()
        );
        // Distinct variants with the same leading param differ.
        assert_ne!(
            ClipEffect::Exposure { stops: 1.0 }.param_hash(),
            ClipEffect::Gamma { power: 1.0 }.param_hash()
        );

        // Stack order changes the fold.
        let a = ClipEffect::Exposure { stops: 1.0 };
        let b = ClipEffect::Gamma { power: 2.0 };
        let forward = EffectStack {
            effects: vec![a, b],
        };
        let reversed = EffectStack {
            effects: vec![b, a],
        };
        assert_ne!(forward.param_hash(), reversed.param_hash());
        assert_ne!(forward.param_hash(), EffectStack::new().param_hash());
    }

    #[test]
    fn animated_effect_resolves_keyframed_curve() {
        let mut stops = Automation::constant(0.0);
        stops.add(Keyframe {
            time_ms: 0,
            value: 0.0,
            interpolation: Interpolation::Linear,
        });
        stops.add(Keyframe {
            time_ms: 1000,
            value: 2.0,
            interpolation: Interpolation::Linear,
        });
        let effect = AnimatedEffect::Exposure { stops };

        let stops_at = |time_ms| match effect.resolve(time_ms) {
            ClipEffect::Exposure { stops } => stops,
            other => panic!("expected exposure, got {other:?}"),
        };
        assert!((stops_at(0) - 0.0).abs() < 1e-6);
        assert!((stops_at(500) - 1.0).abs() < 1e-6);
        assert!((stops_at(1000) - 2.0).abs() < 1e-6);
        // The curve holds the last keyframe past its end.
        assert!((stops_at(5000) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn animated_effect_stack_resolves_to_a_static_stack() {
        let stack = AnimatedEffectStack {
            effects: vec![
                AnimatedEffect::Exposure {
                    stops: Automation::constant(1.0),
                },
                AnimatedEffect::GaussianBlur {
                    sigma: Automation::constant(2.0),
                },
            ],
        };
        assert_eq!(stack.len(), 2);
        let resolved = stack.resolve(250);
        assert_eq!(
            resolved.effects,
            vec![
                ClipEffect::Exposure { stops: 1.0 },
                ClipEffect::GaussianBlur { sigma: 2.0 },
            ]
        );
    }

    #[test]
    fn animated_effect_serde_round_trips() {
        let mut amount = Automation::constant(0.5);
        amount.add(Keyframe {
            time_ms: 100,
            value: 1.0,
            interpolation: Interpolation::Smooth,
        });
        let effect = AnimatedEffect::Saturation { amount };
        let json = serde_json::to_string(&effect).unwrap();
        let restored: AnimatedEffect = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, effect);
    }

    #[test]
    fn non_finite_effect_controls_fail_soft_and_hash_the_sanitized_value() {
        let source = Image::filled(1, 1, [0.25, 0.5, 0.75, 1.0]);
        let invalid = [
            ClipEffect::Exposure { stops: f32::NAN },
            ClipEffect::Gamma {
                power: f32::INFINITY,
            },
            ClipEffect::Saturation {
                amount: f32::NEG_INFINITY,
            },
            ClipEffect::WhiteBalance {
                temperature: f32::NAN,
                tint: f32::NAN,
            },
            ClipEffect::Vignette {
                amount: f32::NAN,
                radius: f32::NAN,
                softness: f32::NAN,
            },
            ClipEffect::Solarize {
                threshold: f32::NAN,
            },
        ];
        for effect in invalid {
            let output = EffectStack {
                effects: vec![effect],
            }
            .apply(&source);
            assert!(output.pixels[0].iter().all(|channel| channel.is_finite()));
        }
        assert_eq!(
            ClipEffect::Exposure { stops: f32::NAN }.param_hash(),
            ClipEffect::Exposure { stops: 0.0 }.param_hash()
        );
        assert_eq!(
            ClipEffect::Exposure { stops: -0.0 }.param_hash(),
            ClipEffect::Exposure { stops: 0.0 }.param_hash()
        );
    }
}

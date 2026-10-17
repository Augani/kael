//! Corpus coverage tests for known Lottie fixtures.
#![allow(clippy::panic, reason = "this is test")]

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use rasterlottie::{
        Animation, RenderConfig, Renderer, SupportProfile, analyze_animation_with_profile,
    };

    #[derive(serde::Deserialize)]
    struct CorpusEntry {
        file_name: String,
        source_url: String,
    }

    #[test]
    fn target_corpus_supports_and_renders_each_animation() {
        for entry in corpus_entries() {
            let _ = &entry.source_url;
            let animation = load_animation(&entry.file_name);
            let report =
                analyze_animation_with_profile(&animation, target_corpus_profile(&entry.file_name));
            assert!(
                report.is_supported(),
                "target corpus animation {} is unsupported: {}",
                entry.file_name,
                report
            );

            Renderer::new(target_corpus_profile(&entry.file_name))
                .render_frame(&animation, 0.0, RenderConfig::default())
                .unwrap_or_else(|error| {
                    panic!(
                        "failed to render target corpus animation {}: {error}",
                        entry.file_name
                    )
                });
        }
    }

    fn corpus_entries() -> Vec<CorpusEntry> {
        let path = corpus_manifest_path();
        let json = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read corpus manifest {}: {error}", path.display())
        });
        serde_json::from_str(&json).unwrap_or_else(|error| {
            panic!(
                "failed to parse corpus manifest {}: {error}",
                path.display()
            )
        })
    }

    fn load_animation(file_name: &str) -> Animation {
        let path = corpus_path(file_name);
        let json = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read corpus file {}: {error}", path.display())
        });
        Animation::from_json_str(&json).unwrap_or_else(|error| {
            panic!("failed to parse corpus file {}: {error}", path.display())
        })
    }

    fn target_corpus_profile(file_name: &str) -> SupportProfile {
        let mut profile = SupportProfile::target_corpus();
        if file_name.contains("image") {
            profile = profile.with_external_image_assets(true);
        }
        profile
    }

    fn corpus_manifest_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("corpus")
            .join("manifest.json")
    }

    fn corpus_path(file_name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("corpus")
            .join(file_name)
    }
}

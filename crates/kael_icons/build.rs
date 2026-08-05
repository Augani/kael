#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing manifest dir"));
    let icon_dir = manifest_dir.join("icons");
    println!("cargo:rerun-if-changed={}", icon_dir.display());

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let generated_path = out_dir.join("generated_icon_catalog.rs");
    let icons = load_icons(&icon_dir);
    let source = generate_catalog(&icons);

    fs::write(generated_path, source).expect("failed to write generated icon catalog");
}

#[derive(Debug)]
struct IconAsset {
    slug: String,
    variant_name: String,
}

fn load_icons(icon_dir: &Path) -> Vec<IconAsset> {
    let mut icons = fs::read_dir(icon_dir)
        .expect("failed to read icon asset directory")
        .map(|entry| entry.expect("failed to read an icon asset directory entry"))
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("svg"))
        .map(|path| {
            println!("cargo:rerun-if-changed={}", path.display());
            let slug = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("icon asset must have a valid UTF-8 file name")
                .to_string();
            validate_slug(&slug);
            let svg = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            validate_svg(&path, &svg);
            IconAsset {
                variant_name: slug_to_variant_name(&slug),
                slug,
            }
        })
        .collect::<Vec<_>>();

    icons.sort_by(|left, right| left.slug.cmp(&right.slug));
    assert!(!icons.is_empty(), "icon catalog must not be empty");
    let mut variants = HashSet::new();
    for icon in &icons {
        assert!(
            variants.insert(icon.variant_name.as_str()),
            "icon slugs generate a duplicate Rust variant: {}",
            icon.variant_name
        );
    }
    icons
}

fn validate_slug(slug: &str) {
    let mut bytes = slug.bytes();
    assert!(
        bytes.next().is_some_and(|byte| byte.is_ascii_lowercase()),
        "icon slug must start with an ASCII lowercase letter: {slug:?}"
    );
    assert!(
        bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "icon slug contains unsupported characters: {slug:?}"
    );
    assert!(
        !slug.ends_with('_') && !slug.contains("__"),
        "icon slug contains an empty segment: {slug:?}"
    );
}

fn validate_svg(path: &Path, svg: &str) {
    assert!(
        svg.contains("<svg"),
        "{} is not an SVG document",
        path.display()
    );
    assert!(
        svg.contains("viewBox="),
        "{} must define a viewBox",
        path.display()
    );
    assert!(
        svg.contains("currentColor"),
        "{} must use currentColor for theme tinting",
        path.display()
    );
    assert!(
        !svg.to_ascii_lowercase().contains("<script"),
        "{} must not contain scripts",
        path.display()
    );
}

fn generate_catalog(icons: &[IconAsset]) -> String {
    let mut source = String::new();
    source.push_str("/// The generated icon names available in this crate.\n");
    source.push_str("pub const GENERATED_ICON_NAMES: &[&str] = &[\n");
    for icon in icons {
        source.push_str(&format!("    \"{}\",\n", icon.slug));
    }
    source.push_str("];\n\n");

    source.push_str("/// A generated icon name.\n");
    source.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    source.push_str("pub enum IconName {\n");
    for icon in icons {
        source.push_str(&format!("    /// The `{}` icon.\n", icon.slug));
        source.push_str(&format!("    {},\n", icon.variant_name));
    }
    source.push_str("}\n\n");

    source.push_str("/// Metadata describing a generated icon asset.\n");
    source.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    source.push_str("pub struct IconMetadata {\n");
    source.push_str("    /// The typed icon name.\n");
    source.push_str("    pub name: IconName,\n");
    source.push_str("    /// The canonical slug used for this icon asset.\n");
    source.push_str("    pub slug: &'static str,\n");
    source.push_str("    /// The SVG source bundled for this icon asset.\n");
    source.push_str("    pub svg: &'static str,\n");
    source.push_str("}\n\n");

    source.push_str("impl IconName {\n");
    source.push_str("    /// Every icon name in deterministic catalog order.\n");
    source.push_str("    pub const ALL: &'static [Self] = &[\n");
    for icon in icons {
        source.push_str(&format!("        Self::{},\n", icon.variant_name));
    }
    source.push_str("    ];\n\n");
    source.push_str("    /// Returns the canonical slug for this icon.\n");
    source.push_str("    pub const fn slug(self) -> &'static str {\n");
    source.push_str("        match self {\n");
    for icon in icons {
        source.push_str(&format!(
            "            Self::{} => \"{}\",\n",
            icon.variant_name, icon.slug
        ));
    }
    source.push_str("        }\n");
    source.push_str("    }\n\n");

    source.push_str("    /// Returns the bundled SVG source for this icon.\n");
    source.push_str("    pub const fn svg(self) -> &'static str {\n");
    source.push_str("        match self {\n");
    for icon in icons {
        source.push_str(&format!(
            "            Self::{} => include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/icons/{}.svg\")),\n",
            icon.variant_name, icon.slug
        ));
    }
    source.push_str("        }\n");
    source.push_str("    }\n\n");

    source.push_str("    /// Resolves an icon from its canonical slug.\n");
    source.push_str("    pub fn from_slug(slug: &str) -> Option<Self> {\n");
    source.push_str("        match slug {\n");
    for icon in icons {
        let kebab_slug = icon.slug.replace('_', "-");
        if kebab_slug == icon.slug {
            source.push_str(&format!(
                "            \"{}\" => Some(Self::{}),\n",
                icon.slug, icon.variant_name
            ));
        } else {
            source.push_str(&format!(
                "            \"{}\" | \"{}\" => Some(Self::{}),\n",
                icon.slug, kebab_slug, icon.variant_name
            ));
        }
    }
    source.push_str("            _ => None,\n");
    source.push_str("        }\n");
    source.push_str("    }\n");
    source.push_str("}\n\n");

    source.push_str("impl std::fmt::Display for IconName {\n");
    source.push_str("    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str(self.slug()) }\n");
    source.push_str("}\n\n");

    source.push_str(
        "/// Error returned when an icon slug is not present in the generated catalog.\n",
    );
    source.push_str("#[derive(Debug, Clone, PartialEq, Eq)]\n");
    source.push_str("pub struct UnknownIconName(String);\n\n");
    source.push_str("impl UnknownIconName {\n");
    source.push_str("    /// Returns the unresolved slug.\n");
    source.push_str("    pub fn slug(&self) -> &str { &self.0 }\n");
    source.push_str("}\n\n");
    source.push_str("impl std::fmt::Display for UnknownIconName {\n");
    source.push_str("    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(formatter, \"unknown icon slug: {:?}\", self.0) }\n");
    source.push_str("}\n\n");
    source.push_str("impl std::error::Error for UnknownIconName {}\n\n");
    source.push_str("impl std::str::FromStr for IconName {\n");
    source.push_str("    type Err = UnknownIconName;\n");
    source.push_str("    fn from_str(slug: &str) -> Result<Self, Self::Err> { Self::from_slug(slug).ok_or_else(|| UnknownIconName(slug.to_string())) }\n");
    source.push_str("}\n\n");

    source.push_str("/// The generated icon metadata bundled in this crate.\n");
    source.push_str("pub const GENERATED_ICONS: &[IconMetadata] = &[\n");
    for icon in icons {
        source.push_str(&format!(
            "    IconMetadata {{ name: IconName::{}, slug: \"{}\", svg: include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/icons/{}.svg\")) }},\n",
            icon.variant_name, icon.slug, icon.slug
        ));
    }
    source.push_str("];\n");

    source
}

fn slug_to_variant_name(slug: &str) -> String {
    slug.split('_')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut characters = segment.chars();
            let first = characters
                .next()
                .map(|character| character.to_ascii_uppercase())
                .unwrap_or('X');
            let rest = characters.collect::<String>();
            format!("{first}{rest}")
        })
        .collect::<String>()
}

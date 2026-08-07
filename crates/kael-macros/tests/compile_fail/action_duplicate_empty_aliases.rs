use kael_macros::Action;

#[derive(Action)]
#[action(
    no_json,
    no_register,
    deprecated_aliases = [],
    deprecated_aliases = []
)]
struct DuplicateAliases;

fn main() {}

use kael_macros::derive_inspector_reflection;

#[derive_inspector_reflection(unexpected)]
trait Invalid {}

fn main() {}

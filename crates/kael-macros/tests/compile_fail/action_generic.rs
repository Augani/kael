use kael_macros::Action;

#[derive(Action)]
struct GenericAction<T>(T);

fn main() {}

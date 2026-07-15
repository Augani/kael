use kael_macros::AppContext;
use kael_renamed::App;

#[derive(AppContext)]
struct DuplicateApp<'a> {
    #[app]
    first: &'a mut App,
    #[app]
    second: &'a mut App,
}

fn main() {}

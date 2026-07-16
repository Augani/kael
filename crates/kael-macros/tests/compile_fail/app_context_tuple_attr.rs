use kael_macros::AppContext;
use kael_renamed::App;

#[derive(AppContext)]
struct TupleApp<'a>(#[app] &'a mut App);

fn main() {}

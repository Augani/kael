use kael_macros::AppContext;

#[derive(AppContext)]
struct MyContext<'a> {
    app: &'a mut u8,
}

fn main() {}

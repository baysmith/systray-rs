extern crate systray;

//#[cfg(target_os = "windows")]
fn main() {
    let mut app;
    match systray::Application::new() {
        Ok(w) => app = w,
        Err(e) => panic!("Can't create window! {}", e),
    }
    app.set_icon_from_file("resources\\rust.ico").ok();
    app.set_tooltip("Whatever").ok();
    app.add_menu_item(0, "Print a thing", None, |_| {
        println!("Printing a thing!");
    })
    .ok();
    app.add_menu_item(0, "Add Menu Item", None, |window| {
        window
            .add_menu_item(0, "Interior item", None, |_| {
                println!("what");
            })
            .ok();
        window.add_menu_separator(0).ok();
    })
    .ok();
    app.add_menu_separator(0).ok();
    app.add_menu_item(0, "Quit", None, |window| {
        window.quit();
    })
    .ok();
    println!("Waiting on message!");
    app.wait_for_message();
}

// #[cfg(not(target_os = "windows"))]
// fn main() {
//     panic!("Not implemented on this platform!");
// }

pub fn show_about() {
    let about = include_str!("../../about.txt");
    println!("{}", about);
}

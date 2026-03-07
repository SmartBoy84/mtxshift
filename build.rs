fn main() {
    println!(
        "cargo:rustc-env=MY_TOKEN={}",
        std::env::var("MY_TOKEN").expect("MY_TOKEN not specified")
    );
}

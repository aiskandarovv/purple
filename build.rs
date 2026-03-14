fn main() {
    let now = time_now();
    println!("cargo:rustc-env=PURPLE_BUILD_DATE={}", now);
}

fn time_now() -> String {
    // UTC date in YYYY-MM-DD format without external crates
    let output = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .expect("failed to run date");
    String::from_utf8(output.stdout)
        .expect("invalid utf8")
        .trim()
        .to_string()
}

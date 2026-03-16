fn main() {
    let now = time_now();
    println!("cargo:rustc-env=PURPLE_BUILD_DATE={}", now);
}

fn time_now() -> String {
    // UTC date in "DD Mon YYYY" format (unambiguous across locales)
    let output = std::process::Command::new("date")
        .args(["-u", "+%-d %b %Y"])
        .output()
        .expect("failed to run date");
    String::from_utf8(output.stdout)
        .expect("invalid utf8")
        .trim()
        .to_string()
}

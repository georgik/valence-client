fn main() {
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");

    // Force frame pointers to be used
    println!("cargo:rustc-env=RUSTFLAGS=-C force-frame-pointers=yes");
}

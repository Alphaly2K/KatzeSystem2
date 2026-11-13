fn main() {
    println!("cargo:rerun-if-changed=src/movie/decoder.c");
    println!("cargo:rerun-if-changed=vendor/pl_mpeg/pl_mpeg.h");
    cc::Build::new()
        .file("src/movie/decoder.c")
        .include("vendor/pl_mpeg")
        .opt_level(2)
        .warnings(false)
        .compile("katze_mpeg");
}

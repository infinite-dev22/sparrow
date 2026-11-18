fn main() {
    // include_dir embeds contents; directory tracking also catches additions and removals.
    println!("cargo:rerun-if-changed=resources/scaffolds");
}

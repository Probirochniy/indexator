fn main() {
    println!("cargo:rerun-if-changed=../proto/erc20.proto");
}
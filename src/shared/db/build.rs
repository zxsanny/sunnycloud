// `sqlx::migrate!` embeds the files of src/migrations/ at compile time; a new or changed file must rebuild the crate.
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
